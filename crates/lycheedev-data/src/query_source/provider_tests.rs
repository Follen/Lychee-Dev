//! Current owned physical cursor boundary over a real authenticated WDC table.
//! The retired borrowed RowProvider surface is covered by the production
//! demand adapter (OwnedPhysicalCursor is its only physical cursor).
use super::*;
use lycheedev_core::control::TrackedCancellation;

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
fn physical(raw: Vec<u8>, definition: &schema::Definition, budget: &Arc<Budget>) -> Arc<Physical<'static>> {
    Arc::new(
        Physical::new(
            table::open_owned_budgeted(
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
        .unwrap(),
    )
}
fn id(row: &[Scalar]) -> u32 {
    match row[0] {
        Scalar::UInt(value) => u32::try_from(value).unwrap(),
        Scalar::Int(value) => u32::try_from(value).unwrap(),
        ref other => panic!("unexpected physical id scalar: {other:?}"),
    }
}

#[test]
fn owned_cursor_scan_preserves_sorted_copy_ids_and_exact_rows() {
    let raw = fixture();
    let d = definition();
    let b = budget();
    let p = physical(raw, &d, &b);
    let before = b.snapshot().unwrap().retained_bytes;
    let mut cursor = p.owned_cursor().unwrap();
    assert!(b.snapshot().unwrap().retained_bytes > before);
    let mut rows = vec![];
    while cursor.advance().unwrap() {
        rows.push((id(cursor.row()), cursor.row()[1].clone()));
        assert_eq!(cursor.current_id(), Some(id(cursor.row())));
    }
    assert_eq!(
        rows.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        [10, 20, 100, 101]
    );
    assert_eq!(rows[0].1, Scalar::Int(11));
    assert_eq!(rows[1].1, Scalar::Int(22));
    assert_eq!(rows[2].1, Scalar::Int(11));
    assert_eq!(rows[3].1, Scalar::Int(11));
    assert_eq!(cursor.current_id(), None);
    drop(cursor);
    assert_eq!(b.snapshot().unwrap().retained_bytes, before);
}

#[test]
fn owned_id_cursor_reads_only_the_requested_exact_row() {
    let raw = fixture();
    let d = definition();
    let b = budget();
    let p = physical(raw, &d, &b);
    let before = b.snapshot().unwrap().retained_bytes;
    for wanted in [10u32, 100, 101] {
        let mut cursor = p.owned_id_cursor(wanted).unwrap();
        assert_eq!(cursor.current_id(), None);
        assert!(cursor.advance().unwrap());
        assert_eq!(id(cursor.row()), wanted);
        assert_eq!(cursor.row()[1], Scalar::Int(11));
        assert_eq!(cursor.current_id(), Some(wanted));
        assert!(!cursor.advance().unwrap());
        assert_eq!(cursor.current_id(), None);
    }
    let mut missing = p.owned_id_cursor(999).unwrap();
    assert!(!missing.advance().unwrap());
    assert_eq!(missing.current_id(), None);
    drop(missing);
    assert_eq!(b.snapshot().unwrap().retained_bytes, before);
}

#[test]
fn owned_cursor_terminal_advance_does_not_recheck_a_cancelled_budget() {
    let raw = fixture();
    let d = definition();
    let token = TrackedCancellation::new();
    let b = Arc::new(Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        Some(token.clone()),
    ));
    let p = physical(raw.clone(), &d, &b);
    let mut cursor = p.owned_cursor().unwrap();
    for _ in 0..4 {
        assert!(cursor.advance().unwrap());
    }
    token.cancel();
    assert_eq!(cursor.advance().unwrap_err().code, "operation.cancelled");
    drop(cursor);

    // An already admitted exact lookup reaches its terminal advance before any
    // trailing budget check: cancellation cannot turn completion into failure.
    let token = TrackedCancellation::new();
    let b = Arc::new(Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        Some(token.clone()),
    ));
    let p = physical(raw.clone(), &d, &b);
    let before = b.snapshot().unwrap().retained_bytes;
    let mut cursor = p.owned_id_cursor(10).unwrap();
    assert!(cursor.advance().unwrap());
    token.cancel();
    assert!(!cursor.advance().unwrap());
    drop(cursor);
    drop(p);
    assert_eq!(b.snapshot().unwrap().retained_bytes, before);
}
