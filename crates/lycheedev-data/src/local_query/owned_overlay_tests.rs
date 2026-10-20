//! Production owned-cursor contracts. Native execution is recorded separately.
use super::*;
use provider::{OwnedEquality, Physical};
use relational::{RowCursor, Scalar, Source};
use std::sync::atomic::{AtomicBool, Ordering};

#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod fixture;

fn budget(cancel: Option<Arc<AtomicBool>>) -> Arc<Budget> {
    Arc::new(Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(60),
        cancel,
    ))
}
fn physical(rows: &[(u32, bool)], budget: &Arc<Budget>) -> Arc<Physical<'static>> {
    let rows = rows
        .iter()
        .map(|&(id, bad)| {
            vec![
                fixture::Cell::U(id),
                if bad {
                    fixture::Cell::BadString
                } else {
                    fixture::Cell::Text(format!("value-{id}"))
                },
            ]
        })
        .collect::<Vec<_>>();
    let fixture = fixture::table("Sample", 42, &[("ID", "uint"), ("Name", "text")], &rows);
    let definition = schema::Document::parse(fixture.dbd.as_bytes())
        .unwrap()
        .select(fixture::BUILD, &format!("{:08X}", fixture.layout))
        .unwrap();
    Arc::new(
        Physical::new(
            table::open_owned_budgeted(
                fixture.raw,
                &definition,
                Default::default(),
                &Default::default(),
                &[],
                budget.clone(),
            )
            .unwrap(),
            budget.clone(),
        )
        .unwrap(),
    )
}
fn plan(patches: &[(u32, bool)]) -> Arc<OverlayPlan> {
    Arc::new(OverlayPlan {
        schema: Source {
            columns: [
                "ID",
                "Name",
                "__hotfix_source",
                "__hotfix_push",
                "__hotfix_index",
            ]
            .map(str::to_owned)
            .to_vec(),
            rows: vec![],
            complete: true,
        },
        identity_index: 0,
        capture_ids: vec!["CAP-owned-fixture".into()],
        patches: patches
            .iter()
            .map(|&(id, deleted)| {
                (
                    id,
                    Patch {
                        values: (!deleted).then(|| {
                            vec![
                                Scalar::UInt(id.into()),
                                Scalar::String(format!("patch-{id}")),
                            ]
                        }),
                        ordinal: 0,
                        entry: CacheEntry {
                            index: id,
                            region: None,
                            push: 1,
                            unique_id: None,
                            table_hash: 42,
                            record_id: id,
                            status: u8::from(!deleted),
                            payload_offset: 0,
                            payload_bytes: 8,
                            payload_hex: String::new(),
                            fields: None,
                            decode_state: String::new(),
                        },
                    },
                )
            })
            .collect(),
    })
}
fn id(cursor: &dyn RowCursor) -> u32 {
    row_id(&cursor.row()[0]).unwrap()
}
fn decoded() -> usize {
    provider::WDC_ROWS.with(std::cell::Cell::get)
}
fn reset_decoded() {
    provider::WDC_ROWS.with(|count| count.set(0));
}

#[test]
fn owned_overlay_merges_before_between_after_and_u32_max_without_lookahead() {
    let budget = budget(None);
    let base = physical(&[(10, false), (20, false), (40, false)], &budget);
    let plan = plan(&[
        (1, false),
        (10, false),
        (15, false),
        (20, true),
        (30, true),
        (50, false),
        (u32::MAX, false),
    ]);
    reset_decoded();
    let mut cursor = plan.owned_cursor(base.clone(), budget.clone()).unwrap();
    // Dropping caller-owned sources cannot invalidate the suspended stream.
    drop(plan);
    drop(base);
    assert_eq!(decoded(), 0);
    let expected = [(1, 0), (10, 1), (15, 1), (40, 3), (50, 3), (u32::MAX, 3)];
    for (wanted, decoded_rows) in expected {
        assert!(cursor.advance().unwrap());
        assert_eq!(id(&cursor), wanted);
        assert_eq!(cursor.current_id(), Some(wanted));
        assert_eq!(
            decoded(),
            decoded_rows,
            "no speculative base row for {wanted}"
        );
        assert_eq!(
            cursor.row()[1],
            Scalar::String(format!(
                "{}-{wanted}",
                if wanted == 40 { "value" } else { "patch" }
            ))
        );
        if wanted == 40 {
            assert_eq!(
                &cursor.row()[2..],
                &[Scalar::Null, Scalar::Null, Scalar::Null]
            );
        } else {
            assert_eq!(cursor.row()[2], Scalar::String("CAP-owned-fixture".into()));
        }
    }
    assert!(!cursor.advance().unwrap());
    assert_eq!(cursor.current_id(), None);
    assert!(!cursor.advance().unwrap());
    drop(cursor);
    assert_eq!(budget.refundable_retained_bytes(), 0);
}

#[test]
fn owned_overlay_bad_tail_is_not_decoded_before_it_is_requested() {
    for deleted in [false, true] {
        let budget = budget(None);
        let base = physical(&[(10, false), (20, true)], &budget);
        let plan = plan(&[(1, false), (15, false), (20, deleted), (30, false)]);
        reset_decoded();
        let mut cursor = plan.owned_cursor(base, budget.clone()).unwrap();
        for expected in [1, 10, 15] {
            assert!(cursor.advance().unwrap());
            assert_eq!(id(&cursor), expected);
            assert_eq!(cursor.current_id(), Some(expected));
        }
        assert_eq!(decoded(), 1);
        // A LIMIT consumer can stop here successfully, before bad base ID 20.
        drop(cursor);
        assert_eq!(budget.refundable_retained_bytes(), 0);
    }
}

#[test]
fn owned_overlay_replace_and_delete_still_validate_all_visited_base_fields() {
    for deleted in [false, true] {
        let budget = budget(None);
        let base = physical(&[(10, false), (20, true)], &budget);
        let plan = plan(&[(15, false), (20, deleted), (30, false)]);
        reset_decoded();
        let mut cursor = plan.owned_cursor(base, budget.clone()).unwrap();
        assert!(cursor.advance().unwrap());
        assert_eq!(id(&cursor), 10);
        assert!(cursor.advance().unwrap());
        assert_eq!(id(&cursor), 15);
        assert_eq!(decoded(), 1);
        // Even if DataFusion projects only ID, the visited Name must validate.
        assert!(cursor.advance().is_err());
        assert_eq!(decoded(), 2);
        assert!(cursor.row().is_empty());
        assert_eq!(cursor.current_id(), None);
        assert!(!cursor.advance().unwrap());
        drop(cursor);
        assert_eq!(budget.refundable_retained_bytes(), 0);
    }
}

#[test]
fn owned_overlay_equality_is_patch_first_even_when_base_is_malformed() {
    let budget = budget(None);
    let base = physical(&[(10, true), (20, true), (40, false)], &budget);
    let plan = plan(&[(10, false), (20, true), (u32::MAX, false)]);
    reset_decoded();
    for wanted in [10, u32::MAX] {
        let OwnedEquality::Rows(mut cursor) = plan
            .owned_equal_cursor(
                base.clone(),
                budget.clone(),
                "id",
                &Scalar::UInt(wanted.into()),
            )
            .unwrap()
        else {
            panic!("replacement cursor")
        };
        assert!(cursor.advance().unwrap());
        assert_eq!(id(&cursor), wanted);
        assert_eq!(cursor.current_id(), Some(wanted));
        assert!(!cursor.advance().unwrap());
    }
    assert!(matches!(
        plan.owned_equal_cursor(base.clone(), budget.clone(), "ID", &Scalar::UInt(20))
            .unwrap(),
        OwnedEquality::Empty
    ));
    assert_eq!(decoded(), 0);
    let OwnedEquality::Rows(mut cursor) = plan
        .owned_equal_cursor(base.clone(), budget.clone(), "ID", &Scalar::UInt(40))
        .unwrap()
    else {
        panic!("base cursor")
    };
    assert!(cursor.advance().unwrap());
    assert_eq!(id(&cursor), 40);
    assert_eq!(decoded(), 1);
    drop(cursor);
    assert!(matches!(
        plan.owned_equal_cursor(base.clone(), budget.clone(), "Name", &Scalar::Bool(true))
            .unwrap(),
        OwnedEquality::Declined
    ));
    assert!(matches!(
        plan.owned_equal_cursor(base.clone(), budget.clone(), "ID", &Scalar::Null)
            .unwrap(),
        OwnedEquality::Empty
    ));
    assert!(
        plan.owned_equal_cursor(base, budget.clone(), "ID", &Scalar::Float(f64::NAN))
            .is_err()
    );
    assert_eq!(budget.refundable_retained_bytes(), 0);
}

#[test]
fn owned_physical_peek_uses_metadata_and_full_row_validation_stays_lazy() {
    let budget = budget(None);
    let base = physical(&[(10, false), (20, true)], &budget);
    reset_decoded();
    let mut cursor = base.owned_cursor().unwrap();
    drop(base);
    assert_eq!(cursor.current_id(), None);
    assert_eq!(cursor.peek_id(), Some(10));
    assert_eq!(cursor.peek_id(), Some(10));
    assert_eq!(decoded(), 0);
    assert!(cursor.advance().unwrap());
    assert_eq!(id(&cursor), 10);
    assert_eq!(cursor.current_id(), Some(10));
    cursor.clear_row();
    assert_eq!(cursor.current_id(), None);
    assert_eq!(cursor.peek_id(), Some(20));
    assert_eq!(decoded(), 1);
    assert!(cursor.advance().is_err());
    assert!(cursor.row().is_empty());
    assert_eq!(cursor.current_id(), None);
    assert_eq!(cursor.peek_id(), None);
    drop(cursor);
    assert_eq!(budget.refundable_retained_bytes(), 0);
}

#[test]
fn owned_cursor_cancel_and_early_drop_refund_the_original_row_guards() {
    for cancel_after_row in [false, true] {
        let cancel = Arc::new(AtomicBool::new(false));
        let budget = budget(Some(cancel.clone()));
        let base = physical(&[(10, false), (20, false)], &budget);
        let plan = plan(&[(10, false)]);
        let mut cursor = plan.owned_cursor(base, budget.clone()).unwrap();
        let cursor_bytes = budget.refundable_retained_bytes();
        assert!(cursor.advance().unwrap());
        assert!(budget.refundable_retained_bytes() > cursor_bytes);
        if cancel_after_row {
            cancel.store(true, Ordering::Relaxed);
            assert_eq!(cursor.advance().unwrap_err().code, "operation.cancelled");
            assert!(cursor.row().is_empty());
            assert_eq!(cursor.current_id(), None);
            assert!(!cursor.advance().unwrap());
        }
        drop(cursor);
        assert_eq!(budget.refundable_retained_bytes(), 0);
    }
}

#[test]
fn owned_overlay_rejects_a_different_budget_without_transferring_ownership() {
    let source_budget = budget(None);
    let other_budget = budget(None);
    let base = physical(&[(10, false)], &source_budget);
    let plan = plan(&[(10, false)]);
    assert!(
        plan.owned_cursor(base.clone(), other_budget.clone())
            .is_err()
    );
    assert!(
        plan.owned_equal_cursor(base, other_budget.clone(), "ID", &Scalar::UInt(10))
            .is_err()
    );
    assert_eq!(source_budget.refundable_retained_bytes(), 0);
    assert_eq!(other_budget.refundable_retained_bytes(), 0);
}

#[test]
fn owned_production_cursors_are_send_and_static_without_unsafe() {
    fn assert_send_static<T: Send + 'static>() {}
    assert_send_static::<provider::OwnedPhysicalCursor>();
    assert_send_static::<OwnedOverlayCursor>();
}

#[test]
fn owned_overlay_maximum_base_id_can_be_replaced_or_deleted_without_wraparound() {
    for deleted in [false, true] {
        let budget = budget(None);
        let base = physical(&[(10, false), (u32::MAX, false)], &budget);
        let plan = plan(&[(1, false), (u32::MAX, deleted)]);
        let mut cursor = plan.owned_cursor(base, budget.clone()).unwrap();
        let mut ids = vec![];
        while cursor.advance().unwrap() {
            ids.push(id(&cursor));
        }
        assert_eq!(
            ids,
            if deleted {
                vec![1, 10]
            } else {
                vec![1, 10, u32::MAX]
            }
        );
        drop(cursor);
        assert_eq!(budget.refundable_retained_bytes(), 0);
    }
}

#[test]
fn owned_overlay_holds_base_row_guard_independently_of_its_augmented_copy() {
    let budget = budget(None);
    let base = physical(&[(10, false)], &budget);
    let plan = plan(&[(10, false)]);
    let mut cursor = plan.owned_cursor(base, budget.clone()).unwrap();
    let cursor_bytes = budget.refundable_retained_bytes();
    assert!(cursor.advance().unwrap());
    let both_rows = budget.refundable_retained_bytes();
    assert!(both_rows > cursor_bytes);
    assert_eq!(
        cursor.base.as_ref().unwrap().row()[1],
        Scalar::String("value-10".into())
    );
    assert_eq!(cursor.row()[1], Scalar::String("patch-10".into()));
    // Dropping the augmented copy must not refund the live decoded base row.
    cursor.current = None;
    cursor.current_id = None;
    let base_row_bytes = budget.refundable_retained_bytes();
    assert!(base_row_bytes < both_rows);
    assert!(base_row_bytes > cursor_bytes);
    cursor.base.as_mut().unwrap().clear_row();
    assert_eq!(budget.refundable_retained_bytes(), cursor_bytes);
    drop(cursor);
    assert_eq!(budget.refundable_retained_bytes(), 0);
}

#[test]
fn owned_current_locator_is_not_the_signed_public_id_column() {
    let budget = budget(None);
    let fixture = fixture::table(
        "Locator",
        43,
        &[("Locator", "uint"), ("ID", "signed"), ("Name", "text")],
        &[vec![
            fixture::Cell::U(u32::MAX),
            fixture::Cell::I(-7),
            fixture::Cell::Text("base".into()),
        ]],
    );
    let mut definition = schema::Document::parse(fixture.dbd.as_bytes())
        .unwrap()
        .select(fixture::BUILD, &format!("{:08X}", fixture.layout))
        .unwrap();
    definition.fields[0].identity = true;
    definition.fields[1].identity = false;
    let base = Arc::new(
        Physical::new(
            table::open_owned_budgeted(
                fixture.raw,
                &definition,
                Default::default(),
                &Default::default(),
                &[],
                budget.clone(),
            )
            .unwrap(),
            budget.clone(),
        )
        .unwrap(),
    );
    let mut physical = base.owned_cursor().unwrap();
    assert_eq!(physical.current_id(), None);
    assert!(physical.advance().unwrap());
    assert_eq!(physical.row()[1], Scalar::Int(-7));
    assert_eq!(physical.current_id(), Some(u32::MAX));
    assert!(!physical.advance().unwrap());
    assert_eq!(physical.current_id(), None);
    drop(physical);
    let mut physical = base.owned_id_cursor(u32::MAX).unwrap();
    assert_eq!(physical.current_id(), None);
    assert!(physical.advance().unwrap());
    assert_eq!(physical.row()[1], Scalar::Int(-7));
    assert_eq!(physical.current_id(), Some(u32::MAX));
    assert!(!physical.advance().unwrap());
    assert_eq!(physical.current_id(), None);
    drop(physical);
    let mut missing = base.owned_id_cursor(7).unwrap();
    assert!(!missing.advance().unwrap());
    assert_eq!(missing.current_id(), None);
    drop(missing);

    let mut overlay = Arc::try_unwrap(plan(&[(u32::MAX, false)])).ok().unwrap();
    overlay.schema.columns = [
        "Locator",
        "ID",
        "Name",
        "__hotfix_source",
        "__hotfix_push",
        "__hotfix_index",
    ]
    .map(str::to_owned)
    .to_vec();
    overlay.patches.get_mut(&u32::MAX).unwrap().values = Some(vec![
        Scalar::UInt(u32::MAX.into()),
        Scalar::Int(-99),
        Scalar::String("patch".into()),
    ]);
    let overlay = Arc::new(overlay);
    for equality in [false, true] {
        let mut cursor = if equality {
            let OwnedEquality::Rows(cursor) = overlay
                .owned_equal_cursor(
                    base.clone(),
                    budget.clone(),
                    "Locator",
                    &Scalar::UInt(u32::MAX.into()),
                )
                .unwrap()
            else {
                panic!("patch equality")
            };
            cursor
        } else {
            overlay.owned_cursor(base.clone(), budget.clone()).unwrap()
        };
        assert_eq!(cursor.current_id(), None);
        assert!(cursor.advance().unwrap());
        assert_eq!(cursor.row()[1], Scalar::Int(-99));
        assert_eq!(cursor.current_id(), Some(u32::MAX));
        assert!(!cursor.advance().unwrap());
        assert_eq!(cursor.current_id(), None);
    }
    let OwnedEquality::Rows(mut by_id) = overlay
        .owned_id_cursor(base.clone(), budget.clone(), u32::MAX)
        .unwrap()
    else {
        panic!("patch physical lookup")
    };
    assert_eq!(by_id.current_id(), None);
    assert!(by_id.advance().unwrap());
    assert_eq!(by_id.row()[1], Scalar::Int(-99));
    assert_eq!(by_id.current_id(), Some(u32::MAX));
    assert!(!by_id.advance().unwrap());
    assert_eq!(by_id.current_id(), None);
    drop(by_id);
    let mut empty = Arc::try_unwrap(plan(&[])).ok().unwrap();
    empty.schema.columns = overlay.schema.columns.clone();
    let empty = Arc::new(empty);
    let OwnedEquality::Rows(mut cursor) = empty
        .owned_equal_cursor(
            base.clone(),
            budget.clone(),
            "Locator",
            &Scalar::UInt(u32::MAX.into()),
        )
        .unwrap()
    else {
        panic!("base equality")
    };
    assert_eq!(cursor.current_id(), None);
    assert!(cursor.advance().unwrap());
    assert_eq!(cursor.row()[1], Scalar::Int(-7));
    assert_eq!(cursor.current_id(), Some(u32::MAX));
    assert!(!cursor.advance().unwrap());
    assert_eq!(cursor.current_id(), None);
    drop(cursor);
    let OwnedEquality::Rows(mut by_id) = empty
        .owned_id_cursor(base.clone(), budget.clone(), u32::MAX)
        .unwrap()
    else {
        panic!("base physical lookup")
    };
    assert!(by_id.advance().unwrap());
    assert_eq!(by_id.row()[1], Scalar::Int(-7));
    assert_eq!(by_id.current_id(), Some(u32::MAX));
    drop(by_id);
    let deleted = plan(&[(u32::MAX, true)]);
    assert!(matches!(
        deleted
            .owned_id_cursor(base, budget.clone(), u32::MAX)
            .unwrap(),
        OwnedEquality::Empty
    ));
    assert_eq!(budget.refundable_retained_bytes(), 0);
}
