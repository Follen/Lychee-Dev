//! Production source contracts with real authenticated synthetic CASC/DBD/WDC.
//! These tests are not real-client or performance acceptance.
use super::*;
use crate::resource::Cost;
use arrow_array::{Array, FixedSizeListArray, Float32Array, Int64Array, UInt64Array};
use lycheedev_core::{evidence, workspace::data_disk::DataCachePaths};
use serde_json::json;
use std::{
    fs,
    time::{Duration, Instant},
};

#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod fixture;
use fixture::{Cell, Fixture};

fn budget(inv: &Invocation) -> Arc<Budget> {
    Arc::new(Budget::controlled(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(60),
        inv.control.clone(),
    ))
}
fn request(mode: SourceMode, captures: Vec<String>) -> TableRequest {
    TableRequest {
        name: "Sample".into(),
        mode,
        hotfix_captures: captures,
    }
}
fn basic() -> fixture::Table {
    fixture::table(
        "Sample",
        921,
        &[
            ("ID", "uint"),
            ("Value", "signed64"),
            ("A", "array3"),
            ("F", "float"),
        ],
        &[
            vec![
                Cell::U(2),
                Cell::I64(i64::MIN),
                Cell::Array(vec![Cell::U(5), Cell::U(6), Cell::U(7)]),
                Cell::F(-0.0),
            ],
            vec![
                Cell::U(1),
                Cell::I64(9_007_199_254_740_993),
                Cell::Array(vec![Cell::U(1), Cell::U(2), Cell::U(3)]),
                Cell::F(1.5),
            ],
        ],
    )
}
fn cache(fx: &Fixture) -> lycheedev_core::Outcome {
    let mut inv = fx.invocation("data cache prepare", &[]);
    inv.positional = Some("Sample".into());
    crate::generation::dispatch(&inv).unwrap()
}
async fn load(fx: &Fixture, request: &TableRequest) -> (Arc<PreparedSnapshot>, Arc<Budget>) {
    let inv = fx.invocation("data sql", &[]);
    let budget = budget(&inv);
    let snapshot = prepare(&inv, request, budget.clone(), inv.control.clone())
        .await
        .unwrap();
    (snapshot, budget)
}

#[tokio::test(flavor = "current_thread")]
async fn production_source_preserves_native_arrays_exact_integers_floats_and_pin() {
    let fx = Fixture::new(&[basic()]);
    let (snapshot, budget) = load(&fx, &request(SourceMode::Static, vec![])).await;
    assert_eq!(snapshot.pin().id, fx.pin);
    assert_eq!(
        snapshot.provenance()["typedGenerationCache"]["status"],
        "miss"
    );
    assert!(snapshot.complete());
    assert_eq!(snapshot.rows(), 2);
    let batch = &snapshot.batches()[0];
    let values = batch
        .column(1)
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap();
    assert_eq!(values.values().as_ref(), &[9_007_199_254_740_993, i64::MIN]);
    let arrays = batch
        .column(2)
        .as_any()
        .downcast_ref::<FixedSizeListArray>()
        .unwrap();
    assert_eq!(arrays.value_length(), 3);
    assert_eq!(
        arrays
            .values()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap()
            .values()
            .as_ref(),
        &[1, 2, 3, 5, 6, 7]
    );
    assert!(
        batch
            .column(3)
            .as_any()
            .downcast_ref::<Float32Array>()
            .unwrap()
            .value(1)
            .is_sign_negative()
    );
    let info = describe(&snapshot, &budget).unwrap();
    assert_eq!(info.row_count, 2);
    assert_eq!(info.columns[2].name, "A");
    assert!(info.columns[2].data_type.contains("FixedSizeList"));
}

#[tokio::test(flavor = "current_thread")]
async fn production_source_reuses_authenticated_generation_without_changing_values() {
    let fx = Fixture::new(&[basic()]);
    let input = request(SourceMode::Static, vec![]);
    let (before, _) = load(&fx, &input).await;
    cache(&fx);
    let (after, _) = load(&fx, &input).await;
    assert_eq!(after.provenance()["typedGenerationCache"]["status"], "hit");
    assert_eq!(before.batches(), after.batches());
    assert_eq!(
        before.provenance()["sourceSha256"],
        after.provenance()["sourceSha256"]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn production_imported_and_ipc_arrow_clones_hold_original_admissions() {
    for cached in [false, true] {
        let fx = Fixture::new(&[basic()]);
        if cached {
            cache(&fx);
        }
        let (snapshot, budget) = load(&fx, &request(SourceMode::Static, vec![])).await;
        let records = snapshot.batches()[0].slice(0, 1);
        assert!(budget.refundable_retained_bytes() > 0);
        drop(snapshot);
        assert!(
            budget.refundable_retained_bytes() > 0,
            "a cloned Arrow slice must retain the original source admission"
        );
        assert_eq!(
            records
                .column(1)
                .as_any()
                .downcast_ref::<Int64Array>()
                .unwrap()
                .value(0),
            9_007_199_254_740_993
        );
        drop(records);
        assert_eq!(
            budget.refundable_retained_bytes(),
            0,
            "buffer ownership must not form an Arc cycle"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn production_source_missing_body_is_explicit_but_corruption_never_falls_back() {
    let fx = Fixture::new(&[basic()]);
    let output = cache(&fx);
    let path = DataCachePaths::new(&fx.home)
        .object(output.data["object"]["sha256"].as_str().unwrap())
        .unwrap();
    let raw = fs::read(&path).unwrap();
    let mut bad = raw.clone();
    let index = bad.len() / 2;
    bad[index] ^= 1;
    fs::write(&path, &bad).unwrap();
    let inv = fx.invocation("data sql", &[]);
    assert!(
        prepare(
            &inv,
            &request(SourceMode::Static, vec![]),
            budget(&inv),
            inv.control.clone()
        )
        .await
        .is_err()
    );
    fs::write(&path, raw).unwrap();
    fs::remove_file(path).unwrap();
    let (snapshot, _) = load(&fx, &request(SourceMode::Static, vec![])).await;
    assert_eq!(
        snapshot.provenance()["typedGenerationCache"]["reason"],
        "materialized_body_missing"
    );
    assert_eq!(snapshot.rows(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn production_source_refuses_static_hotfix_and_missing_effective_capture_before_io() {
    let inv = Invocation {
        command: "data sql".into(),
        positional: None,
        flags: Default::default(),
        home: "must-not-open-production-source".into(),
        cwd: ".".into(),
        control: Default::default(),
    };
    for input in [
        request(SourceMode::Static, vec!["CAP-forbidden".into()]),
        request(SourceMode::Effective, vec![]),
        request(SourceMode::Effective, vec!["CAP-too-many".into(); 17]),
    ] {
        assert_eq!(
            prepare(&inv, &input, budget(&inv), inv.control.clone())
                .await
                .unwrap_err()
                .code,
            "records.hotfix_identity"
        );
    }
    assert_eq!(
        prepare(
            &inv,
            &request(SourceMode::Static, vec![]),
            budget(&inv),
            OperationControl::default()
        )
        .await
        .unwrap_err()
        .code,
        "records.file_query"
    );
}

fn overlay(fx: &Fixture, hash: u32) -> String {
    let mut raw = b"XFTH".to_vec();
    raw.extend(9u32.to_le_bytes());
    raw.extend(69933u32.to_le_bytes());
    raw.resize(44, 0);
    for (id, status, values) in [
        (1u32, 1u8, Some((100u32, [10u32, 11, 12]))),
        (2, 2, None),
        (4, 1, Some((400, [40, 41, 42]))),
    ] {
        let payload = values
            .map(|(value, list)| {
                let mut payload = Vec::new();
                for value in [id, value, list[0], list[1], list[2], (-0.0f32).to_bits()] {
                    payload.extend(value.to_le_bytes());
                }
                payload
            })
            .unwrap_or_default();
        let mut header = vec![0; 32];
        header[..4].copy_from_slice(b"XFTH");
        for (at, value) in [(8, 1u32), (16, hash), (20, id), (24, payload.len() as u32)] {
            header[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        header[28] = status;
        raw.extend(header);
        raw.extend(payload);
    }
    let capture = evidence::commit_capture(&fx.home, &raw, "application/octet-stream",
        json!({"kind":"hotfix-cache","locator":"fixture:engine-source","snapshot":fx.pin,"dataBuild":fixture::BUILD}), true, false).unwrap();
    capture["id"].as_str().unwrap().to_owned()
}
fn effective_fixture() -> (Fixture, String) {
    let spec = fixture::table(
        "Sample",
        923,
        &[
            ("ID", "uint"),
            ("Value", "uint"),
            ("A", "array3"),
            ("F", "float"),
        ],
        &[
            vec![
                Cell::U(1),
                Cell::U(1),
                Cell::Array(vec![Cell::U(1), Cell::U(2), Cell::U(3)]),
                Cell::F(1.0),
            ],
            vec![
                Cell::U(2),
                Cell::U(2),
                Cell::Array(vec![Cell::U(4), Cell::U(5), Cell::U(6)]),
                Cell::F(2.0),
            ],
            vec![
                Cell::U(3),
                Cell::U(3),
                Cell::Array(vec![Cell::U(7), Cell::U(8), Cell::U(9)]),
                Cell::F(3.0),
            ],
        ],
    );
    let hash = spec.hash;
    let fx = Fixture::new(&[spec]);
    let id = overlay(&fx, hash);
    (fx, id)
}

#[tokio::test(flavor = "current_thread")]
async fn production_effective_applies_replacements_deletions_additions_and_nested_arrays() {
    for cached in [false, true] {
        let (fx, id) = effective_fixture();
        if cached {
            cache(&fx);
        }
        let (snapshot, budget) = load(&fx, &request(SourceMode::Effective, vec![id.clone()])).await;
        assert_eq!(snapshot.rows(), 3);
        assert_eq!(snapshot.provenance()["sourceMode"], "effective");
        assert_eq!(snapshot.provenance()["effective"]["deleted"], 1);
        assert_eq!(snapshot.provenance()["effective"]["replacedOrAdded"], 2);
        let records = snapshot.batches()[0].clone();
        assert_eq!(
            records
                .column(0)
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap()
                .values()
                .as_ref(),
            &[1, 3, 4]
        );
        assert_eq!(
            records
                .column(1)
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap()
                .values()
                .as_ref(),
            &[100, 3, 400]
        );
        let lists = records
            .column(2)
            .as_any()
            .downcast_ref::<FixedSizeListArray>()
            .unwrap();
        assert_eq!(
            lists
                .values()
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap()
                .values()
                .as_ref(),
            &[10, 11, 12, 7, 8, 9, 40, 41, 42]
        );
        assert!(
            records
                .column(3)
                .as_any()
                .downcast_ref::<Float32Array>()
                .unwrap()
                .value(0)
                .is_sign_negative()
        );
        assert_eq!(records.schema().field(4).name(), "__hotfix_source");
        assert!(records.column(4).is_null(1));
        drop(snapshot);
        assert!(budget.refundable_retained_bytes() > 0);
        drop(records);
        assert_eq!(budget.refundable_retained_bytes(), 0);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn production_effective_rejects_duplicate_capture_and_ipc_close_failure() {
    let (fx, id) = effective_fixture();
    let inv = fx.invocation("data sql", &[]);
    assert_eq!(
        prepare(
            &inv,
            &request(SourceMode::Effective, vec![id.clone(), id.clone()]),
            budget(&inv),
            inv.control.clone()
        )
        .await
        .unwrap_err()
        .code,
        "records.hotfix_identity"
    );
    cache(&fx);
    crate::generation::observations::FAIL_CLOSES.with(|n| n.set(1));
    let result = prepare(
        &inv,
        &request(SourceMode::Effective, vec![id]),
        budget(&inv),
        inv.control.clone(),
    )
    .await;
    crate::generation::observations::FAIL_CLOSES.with(|n| n.set(0));
    assert_eq!(result.unwrap_err().code, "storage.close_failed");
}
