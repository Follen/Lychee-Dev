use super::*;
use lycheedev_core::public_json::{self, NumberLayout};
use relational::Scalar;
use std::sync::{Arc, atomic::AtomicBool};

// Frozen original producer, including zero-byte retained charges.
fn original_rows(rows: &[Vec<Scalar>], budget: &Budget) -> Result<NumberLayout> {
    let mut numbers = BTreeMap::new();
    for (row_index, row) in rows.iter().enumerate() {
        budget.check()?;
        let mut cells = BTreeMap::new();
        for (column, value) in row.iter().enumerate() {
            budget.charge(Cost::work(1))?;
            if matches!(value, relational::Scalar::Float(_)) {
                budget.charge(Cost::retained(
                    number_tree_bytes::<usize>(cells.len() + 1)
                        - number_tree_bytes::<usize>(cells.len()),
                ))?;
                cells.insert(column, NumberLayout::Binary64);
            }
        }
        if !cells.is_empty() {
            budget.charge(Cost::retained(
                number_tree_bytes::<usize>(numbers.len() + 1)
                    - number_tree_bytes::<usize>(numbers.len()),
            ))?;
            numbers.insert(row_index, NumberLayout::Indexed(cells));
        }
    }
    Ok(NumberLayout::Indexed(numbers))
}

fn budget_for(limits: Cost, stopped: bool, expired: bool) -> Budget {
    Budget::new(
        limits,
        if expired {
            Instant::now() - Duration::from_secs(1)
        } else {
            Instant::now() + Duration::from_secs(30)
        },
        Some(Arc::new(AtomicBool::new(stopped))),
    )
}
fn compare(rows: &[Vec<Scalar>], limits: Cost, stopped: bool, expired: bool) {
    let old_budget = budget_for(limits, stopped, expired);
    let new_budget = budget_for(limits, stopped, expired);
    let old = original_rows(rows, &old_budget);
    let new = public_row_numbers(rows, &new_budget);
    assert_eq!(
        old_budget.snapshot().unwrap(),
        new_budget.snapshot().unwrap()
    );
    assert_eq!(
        old_budget.logical_snapshot().unwrap(),
        new_budget.logical_snapshot().unwrap()
    );
    match (old, new) {
        (Err(old), Err(new)) => {
            assert_eq!(old.code, new.code);
            assert_eq!(old.message, new.message);
        }
        (Ok(old), Ok(new)) => {
            let positions = |layout: &NumberLayout| {
                let NumberLayout::Indexed(rows) = layout else {
                    panic!()
                };
                rows.iter()
                    .map(|(&row, cells)| {
                        let cells = match cells {
                            NumberLayout::Indexed(cells) => cells,
                            NumberLayout::SharedIndexed(cells) => cells.as_ref(),
                            _ => panic!(),
                        };
                        assert!(
                            cells
                                .values()
                                .all(|cell| matches!(cell, NumberLayout::Binary64))
                        );
                        (row, cells.keys().copied().collect::<Vec<_>>())
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(positions(&old), positions(&new));
            let value = serde_json::to_value(rows).unwrap();
            for pretty in [false, true] {
                let render = |layout: &NumberLayout| {
                    let mut out = Vec::new();
                    let result = if pretty {
                        public_json::write_pretty_line(&mut out, &value, layout)
                    } else {
                        public_json::write_line(&mut out, &value, layout)
                    };
                    (out, result.err().map(|e| e.to_string()))
                };
                let expected = render(&old);
                if rows
                    .iter()
                    .flatten()
                    .all(|value| !matches!(value, Scalar::Float(f) if !f.is_finite()))
                {
                    assert!(
                        expected.1.is_none(),
                        "finite layout must render successfully"
                    );
                }
                assert_eq!(expected, render(&new));
            }
        }
        _ => panic!("original and shared layout admission differ"),
    }
}

#[test]
fn shared_row_numbers_match_full_homogeneous_bytes_and_share_one_owner() {
    let row = vec![
        Scalar::UInt(u64::MAX),
        Scalar::Float(-0.0),
        Scalar::Float(1e-7),
        Scalar::Float(1e21),
        Scalar::Float(1.0),
    ];
    let rows = vec![row; 16_384];
    compare(&rows, Cost::defaults(), false, false);
    let budget = budget_for(Cost::defaults(), false, false);
    let NumberLayout::Indexed(layouts) = public_row_numbers(&rows, &budget).unwrap() else {
        panic!()
    };
    assert_eq!(layouts.len(), rows.len());
    let NumberLayout::SharedIndexed(first) = &layouts[&0] else {
        panic!()
    };
    assert_eq!(Arc::strong_count(first), rows.len());
    for layout in layouts.values() {
        let NumberLayout::SharedIndexed(cells) = layout else {
            panic!()
        };
        assert!(Arc::ptr_eq(first, cells));
    }
    let weak = Arc::downgrade(first);
    drop(layouts);
    assert!(weak.upgrade().is_none());
}

#[test]
fn shared_row_numbers_keep_empty_heterogeneous_and_wide_fallbacks() {
    let f = Scalar::Float(1.25);
    let rows = vec![
        vec![],
        vec![Scalar::Null],
        vec![f.clone(), Scalar::Int(1), f.clone()],
        vec![f.clone()],
        vec![Scalar::Int(2), f.clone(), f.clone()],
        vec![f.clone(), Scalar::String("x".into()), f.clone(), f.clone()],
        vec![Scalar::UInt(9_007_199_254_740_993), Scalar::Bool(true)],
    ];
    compare(&rows, Cost::defaults(), false, false);
    compare(
        &[vec![Scalar::Float(f64::NAN), Scalar::Float(f64::INFINITY)]],
        Cost::defaults(),
        false,
        false,
    );
    // Divergence after a matched prefix must reconstruct only the actual keys.
    compare(
        &[
            vec![
                f.clone(),
                Scalar::Int(0),
                f.clone(),
                Scalar::Int(0),
                f.clone(),
            ],
            vec![
                f.clone(),
                Scalar::Int(0),
                Scalar::String("not a float".into()),
                f.clone(),
                f.clone(),
            ],
        ],
        Cost::defaults(),
        false,
        false,
    );
    compare(&[], Cost::defaults(), false, false);
    for first_width in [8, 9] {
        let mut sparse = vec![Scalar::Int(0); 16_384];
        sparse[16_383] = f.clone();
        let rows = vec![
            vec![],
            vec![f.clone(); first_width],
            vec![f.clone(); 9],
            vec![f.clone(); 7],
            sparse,
        ];
        compare(&rows, Cost::defaults(), false, false);
        let budget = budget_for(Cost::defaults(), false, false);
        let NumberLayout::Indexed(layouts) = public_row_numbers(&rows, &budget).unwrap() else {
            panic!()
        };
        assert_eq!(
            matches!(layouts[&1], NumberLayout::SharedIndexed(_)),
            first_width == 8
        );
        if first_width == 9 {
            assert!(
                layouts
                    .values()
                    .all(|layout| matches!(layout, NumberLayout::Indexed(_)))
            );
        }
    }
}

#[test]
fn shared_row_numbers_preserve_every_work_and_retained_boundary() {
    let f = Scalar::Float(-0.0);
    let rows = vec![
        vec![f.clone(); 8],
        vec![f.clone(); 8],
        vec![f.clone(); 7],
        vec![Scalar::Null, f.clone()],
        vec![f; 9],
        vec![],
    ];
    let baseline = budget_for(Cost::defaults(), false, false);
    original_rows(&rows, &baseline).unwrap();
    let total = baseline.snapshot().unwrap();
    for work in 0..=total.decode_work {
        compare(
            &rows,
            Cost {
                decode_work: work,
                ..Cost::defaults()
            },
            false,
            false,
        );
    }
    let mut boundaries = std::collections::BTreeSet::from([0u64]);
    let (mut retained, mut marked_rows) = (0usize, 0usize);
    for row in &rows {
        let mut floats = 0;
        for value in row {
            if matches!(value, Scalar::Float(_)) {
                retained +=
                    number_tree_bytes::<usize>(floats + 1) - number_tree_bytes::<usize>(floats);
                floats += 1;
                boundaries.insert(retained as u64);
                boundaries.insert(retained.saturating_sub(1) as u64);
            }
        }
        if floats != 0 {
            retained += number_tree_bytes::<usize>(marked_rows + 1)
                - number_tree_bytes::<usize>(marked_rows);
            marked_rows += 1;
            boundaries.insert(retained as u64);
            boundaries.insert(retained.saturating_sub(1) as u64);
        }
    }
    assert_eq!(retained as u64, total.retained_bytes);
    for retained_bytes in boundaries {
        compare(
            &rows,
            Cost {
                retained_bytes,
                ..Cost::defaults()
            },
            false,
            false,
        );
    }
    compare(&rows, Cost::defaults(), true, false);
    compare(&rows, Cost::defaults(), false, true);
    compare(&rows, Cost::default(), true, true);
}

#[test]
fn shared_row_numbers_arc_owner_fits_original_spare_node_allowance() {
    // One template has <=8 entries and fits the current BTree leaf. The
    // original formula reserves >=3 conservative nodes, independently of any
    // savings from later rows. This gate must not be fixed by raising limits.
    let node_bound = 16 * size_of::<(usize, NumberLayout)>() + 128;
    let arc_owner = 2 * size_of::<usize>() + size_of::<BTreeMap<usize, NumberLayout>>();
    for entries in 1..=8 {
        assert!(number_tree_bytes::<usize>(entries) >= node_bound + arc_owner);
    }
}
