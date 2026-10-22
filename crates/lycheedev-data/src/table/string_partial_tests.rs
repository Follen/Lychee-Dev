//! Authenticated unavailable spans must not obscure an already terminated string.
use super::*;
use crate::schema::Document;
use lycheedev_core::control::{OperationControl, TrackedCancellation};
use std::time::{Duration, Instant};
#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "../../tests/support/partial_strings.rs"]
mod strings_fixture;
fn definition(spec: &fixture::Table) -> Definition {
    Document::parse(spec.dbd.as_bytes())
        .unwrap()
        .select(fixture::BUILD, &format!("{:08X}", spec.layout))
        .unwrap()
}
fn view<'a>(spec: &'a fixture::Table, ranges: &[Range<usize>]) -> Table<'a> {
    open_available(
        &spec.raw,
        &definition(spec),
        TableLimits::default(),
        &BTreeSet::from([1]),
        ranges,
    )
    .unwrap()
}
#[test]
fn cross_partition_strings_stop_before_same_block_and_later_missing_spans() {
    for (gap, text) in [
        (16, b"visible\0".to_vec()),
        (512, b"visible\0".to_vec()),
        (512, [vec![b'a'; 300], vec![0]].concat()),
    ] {
        let spec = strings_fixture::table(201, gap, &text);
        let start = strings_fixture::pool(&spec.raw);
        let table = view(&spec, &[start + gap..start + gap + 8]);
        let expected = String::from_utf8(text[..text.len() - 1].to_vec()).unwrap();
        assert_eq!(table.row(1, 1024).unwrap()["Name"], Value::String(expected));
        assert_eq!(table.ids().collect::<Vec<_>>(), vec![1]);
        assert_eq!(table.coverage()["complete"], false);
    }
}
#[test]
fn missing_string_prefix_or_terminator_remains_unavailable_with_unsorted_spans() {
    for gap in [0, 4, 7] {
        let spec = strings_fixture::table(201, gap, b"visible\0");
        let start = strings_fixture::pool(&spec.raw);
        // The public parser accepts unsorted spans; only the nearest relevant gap matters.
        let table = view(
            &spec,
            &[start + 900..start + 908, start + gap..start + gap + 8],
        );
        assert_eq!(
            table.row(1, 1024).unwrap_err().code,
            "table.partition_unavailable"
        );
    }
    let spec = strings_fixture::table(201, 8, b"visible\0");
    let start = strings_fixture::pool(&spec.raw);
    assert_eq!(
        view(&spec, &[start + 8..start + 16]).row(1, 1024).unwrap()["Name"],
        Value::String("visible".into())
    );
}
#[test]
fn terminated_prefix_preserves_utf8_pool_boundaries_and_exact_output_limits() {
    let split_utf8 = [vec![b'a'; 255], "🙂\0".as_bytes().to_vec()].concat();
    let mut spec = strings_fixture::table(201, 512, &split_utf8);
    let start = strings_fixture::pool(&spec.raw);
    let table = view(&spec, &[start + 512..start + 520]);
    let mut allocation = RowAllocation::new(None, false);
    assert_eq!(
        table
            .terminated(start, start + 1024, 259, &mut allocation)
            .unwrap()
            .as_bytes(),
        &split_utf8[..259]
    );
    assert_eq!(
        table
            .terminated(start, start + 1024, 258, &mut allocation)
            .unwrap_err()
            .code,
        "table.limit_exceeded"
    );
    assert_eq!(
        table
            .terminated(start, start, 1024, &mut allocation)
            .unwrap_err()
            .code,
        "table.invalid_format"
    );
    drop(table);
    spec.raw[start..].fill(b'x');
    spec.raw[start] = 0xff;
    spec.raw[start + 1] = 0;
    assert_eq!(
        view(&spec, &[])
            .terminated(start, start + 1024, 1024, &mut allocation)
            .unwrap_err()
            .code,
        "table.invalid_format"
    );
    spec.raw[start..].fill(b'x');
    let table = view(&spec, &[]);
    assert_eq!(
        table
            .terminated(start, start + 1024, 1024, &mut allocation)
            .unwrap_err()
            .code,
        "table.invalid_format"
    );
    assert_eq!(
        table
            .terminated(start, start + 1024, 1022, &mut allocation)
            .unwrap_err()
            .code,
        "table.limit_exceeded"
    );
    drop(table);
    spec.raw[start + 1023] = 0;
    assert_eq!(
        view(&spec, &[])
            .terminated(start, start + 1024, 1023, &mut allocation)
            .unwrap()
            .len(),
        1023
    );
    spec.raw[start] = 0;
    assert_eq!(
        view(&spec, &[])
            .terminated(start, start + 1024, 0, &mut allocation)
            .unwrap(),
        ""
    );
}
#[test]
fn terminated_prefix_keeps_original_control_and_admits_only_returned_bytes() {
    let spec = strings_fixture::table(201, 512, b"visible\0");
    let start = strings_fixture::pool(&spec.raw);
    let ranges = [start + 512..start + 520];
    let mut table = view(&spec, &ranges);
    let ledger = Arc::new(Budget::new(
        Cost {
            retained_bytes: 7,
            ..Cost::defaults()
        },
        Instant::now() + Duration::from_secs(30),
        None,
    ));
    table.source.budget = Some(ledger.clone());
    assert_eq!(
        table
            .terminated(
                start,
                start + 1024,
                1024,
                &mut RowAllocation::new(Some(&ledger), false)
            )
            .unwrap(),
        "visible"
    );
    assert_eq!(ledger.snapshot().unwrap(), Cost::retained(7));
    assert_eq!(
        table
            .terminated(
                start,
                start + 1024,
                1024,
                &mut RowAllocation::new(Some(&ledger), false)
            )
            .unwrap_err()
            .code,
        "records.query_resource_budget"
    );
    assert_eq!(ledger.snapshot().unwrap(), Cost::retained(7));
    let stop = TrackedCancellation::new();
    let control = OperationControl::tracked(Instant::now() + Duration::from_secs(30), stop.clone());
    let ledger = Arc::new(Budget::controlled(
        Cost::defaults(),
        control.deadline(),
        control.clone(),
    ));
    table.source.budget = Some(ledger.clone());
    stop.cancel();
    let error = table
        .terminated(
            start,
            start + 1024,
            1024,
            &mut RowAllocation::new(Some(&ledger), false),
        )
        .unwrap_err();
    assert_eq!(
        serde_json::to_value(error).unwrap(),
        serde_json::to_value(control.check().unwrap_err()).unwrap()
    );
    assert_eq!(ledger.snapshot().unwrap(), Cost::default());
    let blocked = strings_fixture::table(201, 4, b"visible\0");
    let blocked_start = strings_fixture::pool(&blocked.raw);
    let mut blocked_view = view(&blocked, &[blocked_start + 4..blocked_start + 12]);
    let ledger = Arc::new(Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        None,
    ));
    blocked_view.source.budget = Some(ledger.clone());
    assert_eq!(
        blocked_view
            .terminated(
                blocked_start,
                blocked_start + 1024,
                1024,
                &mut RowAllocation::new(Some(&ledger), false)
            )
            .unwrap_err()
            .code,
        "table.partition_unavailable"
    );
    assert_eq!(
        ledger.snapshot().unwrap(),
        Cost::default(),
        "an unterminated available prefix is not a returned allocation or a read through unknown bytes"
    );
}
