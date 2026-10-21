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

#[tokio::test(flavor = "current_thread")]
async fn authenticated_locator_materializes_selected_batches_after_full_validation() {
    let rows = (0..4096)
        .map(|i| vec![Cell::U(i * 2 + 1), Cell::I64(7)])
        .collect::<Vec<_>>();
    let fx = Fixture::new(&[fixture::table(
        "Sample",
        921,
        &[("ID", "uint"), ("Value", "signed64")],
        &rows,
    )]);
    cache(&fx);
    let inv = fx.invocation("data sql", &[]);
    let budget = budget(&inv);
    let snapshot = prepare_selected(
        &inv,
        &request(SourceMode::Static, vec![]),
        Some(&[2, 3, 50_000]),
        budget.clone(),
        inv.control.clone(),
    )
    .await
    .unwrap();
    let report = &snapshot.provenance()["sourceSelection"];
    assert_eq!(report["kind"], "authenticated_id_locator_batches");
    assert_eq!(report["materializationDecodedBatches"], 1);
    assert_eq!(report["missingIds"], json!([2, 50_000]));
    assert_eq!(report["unresolvedIds"], json!([]));
    assert!(snapshot.covers(Some(&[3]), &budget).unwrap());
    assert!(!snapshot.covers(None, &budget).unwrap());
    assert!(snapshot.covers(Some(&[5]), &budget).unwrap());
    assert!(!snapshot.covers(Some(&[4095]), &budget).unwrap());
    assert!(snapshot.rows() < 4096);
}

#[tokio::test(flavor = "current_thread")]
async fn distinct_lookups_share_union_and_later_full_scan_never_reuses_partial_source() {
    let rows = (0..4096)
        .map(|i| vec![Cell::U(i * 2 + 1), Cell::I64(7)])
        .collect::<Vec<_>>();
    let fx = Fixture::new(&[fixture::table(
        "Sample",
        921,
        &[("ID", "uint"), ("Value", "signed64")],
        &rows,
    )]);
    cache(&fx);
    let inv = fx.invocation("data sql", &[]);
    let mut engine =
        crate::engine::Engine::new(inv.clone(), crate::engine::EngineLimits::default()).unwrap();
    let query = serde_json::from_value(json!({"kind":"plan","plan":{"op":"join","left":{"op":"lookup","table":{"name":"Sample","mode":"static"},"ids":[1]},"right":{"op":"lookup","table":{"name":"Sample","mode":"static"},"ids":[3]},"kind":"inner","on":[{"left":"Value","right":"Value"}]}})).unwrap();
    let result = engine.execute(query, inv.control.clone()).await.unwrap();
    assert_eq!(result.row_count(), 1);
    drop(result);
    let query = serde_json::from_value(
        json!({"kind":"plan","plan":{"op":"scan","table":{"name":"Sample","mode":"static"}}}),
    )
    .unwrap();
    let result = engine.execute(query, inv.control.clone()).await.unwrap();
    assert_eq!(result.row_count(), 4096);
    drop(result);
    engine.close().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn selected_effective_ids_resolve_patch_add_delete_before_base_lookup() {
    for cached in [false, true] {
        let (fx, capture) = effective_fixture();
        if cached {
            cache(&fx);
        }
        let inv = fx.invocation("data sql", &[]);
        let snapshot = prepare_selected(
            &inv,
            &request(SourceMode::Effective, vec![capture]),
            Some(&[1, 2, 4, 99]),
            budget(&inv),
            inv.control.clone(),
        )
        .await
        .unwrap();
        assert_eq!(snapshot.rows(), 2);
        let ids = snapshot
            .batches()
            .iter()
            .flat_map(|batch| {
                batch
                    .column(0)
                    .as_any()
                    .downcast_ref::<UInt64Array>()
                    .unwrap()
                    .values()
                    .to_vec()
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![1, 4]);
        let report = &snapshot.provenance()["sourceSelection"];
        assert_eq!(report["kind"], "authenticated_effective_id_lookup");
        assert_eq!(report["missingIds"], json!([2, 99]));
        assert_eq!(report["unresolvedIds"], json!([]));
        assert!(report["materializationDecodedBatches"].is_null());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn uncached_selected_source_imports_only_existing_requested_rows() {
    let fx = Fixture::new(&[basic()]);
    let inv = fx.invocation("data sql", &[]);
    let snapshot = prepare_selected(
        &inv,
        &request(SourceMode::Static, vec![]),
        Some(&[1, 99]),
        budget(&inv),
        inv.control.clone(),
    )
    .await
    .unwrap();
    assert_eq!(snapshot.rows(), 1);
    assert_eq!(
        snapshot.provenance()["sourceSelection"]["kind"],
        "authenticated_wdc_id_lookup"
    );
    assert_eq!(
        snapshot.provenance()["sourceSelection"]["missingIds"],
        json!([99])
    );
    let empty = prepare_selected(
        &inv,
        &request(SourceMode::Static, vec![]),
        Some(&[]),
        budget(&inv),
        inv.control.clone(),
    )
    .await
    .unwrap();
    assert_eq!(empty.rows(), 0);
    assert!(empty.batches().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn same_generation_distinct_lookup_ids_keep_demand_coverage_and_reuse_exact_native_rows() {
    let rows = (0..4096)
        .map(|i| vec![Cell::U(i * 2 + 1), Cell::I64(7)])
        .collect::<Vec<_>>();
    let fx = Fixture::new(&[fixture::table(
        "Sample",
        921,
        &[("ID", "uint"), ("Value", "signed64")],
        &rows,
    )]);
    cache(&fx);
    let inv = fx.invocation("data sql", &[]);
    let mut engine =
        crate::engine::Engine::new(inv.clone(), crate::engine::EngineLimits::default()).unwrap();
    for (index, id) in [1, 3, 3].into_iter().enumerate() {
        let query = serde_json::from_value(json!({"kind":"plan","plan":{"op":"lookup","table":{"name":"Sample","mode":"static"},"ids":[id]}})).unwrap();
        let result = engine.execute(query, inv.control.clone()).await.unwrap();
        assert_eq!(result.row_count(), 1);
        assert_eq!(serde_json::to_value(result.page(0, 10).unwrap()).unwrap()["rows"], json!([[id, 7]]));
        let source = &result.provenance()["sources"][0];
        assert_eq!(source["typedGenerationCache"]["status"], "hit");
        // Demand decoding retains only visited physical rows. Another ID in
        // the same IPC batch needs fresh preparation; an exact later request
        // reuses the resulting native Arrow snapshot with a fresh control.
        let reused = index == 2;
        assert_eq!(source["sessionSnapshot"]["reused"], reused);
        assert_eq!(source["sessionSnapshot"]["sourceReadingScope"], if reused {
            "authenticated_at_snapshot_construction"
        } else {
            "current_request"
        });
        assert_eq!(source["typedGenerationCache"]["representationReason"], if reused {
            "session_native_arrow_reuse"
        } else {
            "demand_work_boundary"
        });
        assert_eq!(result.provenance()["lookupRequirements"][0]["requestedIds"], json!([id]));
    }
    engine.close().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn current_lookup_explanation_does_not_repeat_cached_superset_missing_ids() {
    let fx = Fixture::new(&[basic()]);
    cache(&fx);
    let inv = fx.invocation("data sql", &[]);
    let mut engine =
        crate::engine::Engine::new(inv.clone(), crate::engine::EngineLimits::default()).unwrap();
    for (index, ids) in [vec![1, 99], vec![1], vec![99]].into_iter().enumerate() {
        let query = serde_json::from_value(json!({"kind":"plan","plan":{"op":"lookup","table":{"name":"Sample","mode":"static"},"ids":ids}})).unwrap();
        let result = engine.execute(query, inv.control.clone()).await.unwrap();
        let current = &result.provenance()["lookupRequirements"][0];
        assert_eq!(current["requestedIds"], json!(ids));
        assert_eq!(
            current["missingIds"],
            if ids.contains(&99) {
                json!([99])
            } else {
                json!([])
            }
        );
        assert_eq!(current["unresolvedIds"], json!([]));
        assert_eq!(current["scope"], "current_query_source_requirement");
        let source = &result.provenance()["sources"][0];
        assert_eq!(source["sessionSnapshot"]["reused"], index != 0);
        assert_eq!(source["sessionSnapshot"]["sourceReadingScope"], if index == 0 {
            "current_request"
        } else {
            "authenticated_at_snapshot_construction"
        });
        assert_eq!(result.row_count(), usize::from(ids.contains(&1)));
    }
    engine.close().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn partial_lookup_distinguishes_explicit_hotfix_deletion_from_unresolved_base_id() {
    for effective in [false, true] {
        let spec = fixture::encrypted_sections(201);
        let hash = spec.hash;
        let (_, encoded) = fixture::encrypted_frame(&spec.raw);
        let fx = Fixture::with_encoded_table(spec, encoded);
        let mut table = json!({"name":"Sample","mode":"static"});
        if effective {
            let mut raw = b"XFTH".to_vec();
            raw.extend(9u32.to_le_bytes());
            raw.extend(69933u32.to_le_bytes());
            raw.resize(44, 0);
            let mut header = vec![0; 32];
            header[..4].copy_from_slice(b"XFTH");
            for (at, value) in [(8, 1u32), (16, hash), (20, 2), (24, 0)] {
                header[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
            header[28] = 2;
            raw.extend(header);
            let capture = evidence::commit_capture(&fx.home,&raw,"application/octet-stream",json!({"kind":"hotfix-cache","locator":"fixture:partial-lookup","snapshot":fx.pin,"dataBuild":fixture::BUILD}),true,false).unwrap();
            table = json!({"name":"Sample","mode":"effective","hotfix_captures":[capture["id"]]});
        }
        let inv = fx.invocation("data sql", &[]);
        let mut engine =
            crate::engine::Engine::new(inv.clone(), crate::engine::EngineLimits::default())
                .unwrap();
        let query = serde_json::from_value(
            json!({"kind":"plan","plan":{"op":"lookup","table":table,"ids":[1,2,99]}}),
        )
        .unwrap();
        let result = engine.execute(query, inv.control.clone()).await.unwrap();
        assert!(!result.complete());
        assert_eq!(result.row_count(), 1);
        let report = &result.provenance()["lookupRequirements"][0];
        assert_eq!(
            report["missingIds"],
            if effective { json!([2]) } else { json!([]) }
        );
        assert_eq!(
            report["unresolvedIds"],
            if effective {
                json!([99])
            } else {
                json!([2, 99])
            }
        );
        drop(result);
        engine.close().unwrap();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn selected_source_preserves_cancellation_and_checked_generation_close() {
    let fx = Fixture::new(&[basic()]);
    cache(&fx);
    let inv = fx.invocation("data sql", &[]);
    crate::generation::observations::FAIL_CLOSES.with(|n| n.set(1));
    let result = prepare_selected(
        &inv,
        &request(SourceMode::Static, vec![]),
        Some(&[]),
        budget(&inv),
        inv.control.clone(),
    )
    .await;
    crate::generation::observations::FAIL_CLOSES.with(|n| n.set(0));
    assert_eq!(result.unwrap_err().code, "storage.close_failed");
    inv.control
        .cancel_handle()
        .store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(
        prepare_selected(
            &inv,
            &request(SourceMode::Static, vec![]),
            Some(&[1]),
            budget(&inv),
            inv.control.clone()
        )
        .await
        .is_err()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn sparse_duplicate_lookup_preserves_membership_and_reuses_distinct_demand_rows() {
    let rows = (0..4096)
        .map(|i| vec![Cell::U(i * 2 + 1), Cell::I64(7)])
        .collect::<Vec<_>>();
    let fx = Fixture::new(&[fixture::table(
        "Sample",
        921,
        &[("ID", "uint"), ("Value", "signed64")],
        &rows,
    )]);
    cache(&fx);
    let inv = fx.invocation("data sql", &[]);
    let mut engine =
        crate::engine::Engine::new(inv.clone(), crate::engine::EngineLimits::default()).unwrap();
    let query: crate::engine::QueryRequest = serde_json::from_value(json!({"kind":"plan","plan":{"op":"lookup","table":{"name":"Sample","mode":"static"},"ids":[4097,1,4097,50_000]}})).unwrap();
    let repeated = query.clone();
    let result = engine.execute(query, inv.control.clone()).await.unwrap();
    assert_eq!(result.row_count(), 2);
    let mut rows = serde_json::to_value(result.page(0, 10).unwrap()).unwrap()["rows"].as_array().unwrap().clone();
    rows.sort_by_key(|row| row[0].as_u64().unwrap());
    assert_eq!(rows, vec![json!([1, 7]), json!([4097, 7])]);
    assert_eq!(result.provenance()["sources"][0]["typedGenerationCache"]["representationReason"], "demand_work_boundary");
    assert_eq!(result.provenance()["sources"][0]["sessionSnapshot"]["reused"], false);
    assert_eq!(
        result.provenance()["lookupRequirements"][0]["requestedIds"],
        json!([1, 4097, 50_000])
    );
    assert_eq!(
        result.provenance()["lookupRequirements"][0]["missingIds"],
        json!([50_000])
    );
    drop(result);
    let result = engine.execute(repeated, inv.control.clone()).await.unwrap();
    assert_eq!(result.row_count(), 2);
    let source = &result.provenance()["sources"][0];
    assert_eq!(source["sessionSnapshot"]["reused"], true);
    assert_eq!(source["sessionSnapshot"]["coverage"], "resolved_physical_id_set");
    assert_eq!(source["typedGenerationCache"]["representationSelection"], "authenticated_arrow_session_snapshot");
    assert_eq!(result.provenance()["lookupRequirements"][0]["missingIds"], json!([50_000]));
    drop(result);
    engine.close().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn physical_tombstone_never_proves_absence_of_nonidentity_public_id() {
    let mut spec = fixture::encrypted_sections(201);
    spec.dbd = spec
        .dbd
        .replace("int ID\n", "int RecordID\n")
        .replace("int Value\n", "int ID\n")
        .replace("$id$ID<u32>", "$id$RecordID<u32>")
        .replace("Value<u32>", "ID<u32>");
    let hash = spec.hash;
    let (_, encoded) = fixture::encrypted_frame(&spec.raw);
    let fx = Fixture::with_encoded_table(spec, encoded);
    let mut raw = b"XFTH".to_vec();
    raw.extend(9u32.to_le_bytes());
    raw.extend(69933u32.to_le_bytes());
    raw.resize(44, 0);
    let mut header = vec![0; 32];
    header[..4].copy_from_slice(b"XFTH");
    for (at, value) in [(8, 1u32), (16, hash), (20, 2), (24, 0)] {
        header[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    header[28] = 2;
    raw.extend(header);
    let capture = evidence::commit_capture(&fx.home,&raw,"application/octet-stream",json!({"kind":"hotfix-cache","locator":"fixture:identity-domains","snapshot":fx.pin,"dataBuild":fixture::BUILD}),true,false).unwrap();
    let inv = fx.invocation("data sql", &[]);
    let mut engine =
        crate::engine::Engine::new(inv.clone(), crate::engine::EngineLimits::default()).unwrap();
    let query = serde_json::from_value(json!({"kind":"plan","plan":{"op":"lookup","table":{"name":"Sample","mode":"effective","hotfix_captures":[capture["id"]]},"ids":[2,42]}})).unwrap();
    let result = engine.execute(query, inv.control.clone()).await.unwrap();
    assert_eq!(result.row_count(), 1);
    assert!(!result.complete());
    assert_eq!(result.provenance()["sources"][0]["typedGenerationCache"]["representationSelection"], "authenticated_wdc_demand_stream");
    let report = &result.provenance()["lookupRequirements"][0];
    assert_eq!(report["classification"], "exact_visible_integer_id");
    assert_eq!(report["identityDomain"], "public_id_not_physical_locator");
    assert_eq!(
        result.provenance()["lookupRequirements"][0]["missingIds"],
        json!([])
    );
    assert_eq!(
        result.provenance()["lookupRequirements"][0]["unresolvedIds"],
        json!([2])
    );
    drop(result);
    engine.close().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn nonidentity_integer_lookup_uses_visited_public_values_for_missing_ids() {
    let mut spec = fixture::table(
        "Sample",
        921,
        &[("RecordID", "uint"), ("ID", "uint")],
        &[vec![Cell::U(1), Cell::U(42)], vec![Cell::U(2), Cell::U(43)]],
    );
    spec.dbd = spec.dbd
        .replace("RecordID<u32>", "$id$RecordID<u32>")
        .replace("$id$ID<u32>", "ID<u32>");
    let fx = Fixture::new(&[spec]);
    let inv = fx.invocation("data agent serve", &[]);
    let mut engine = crate::engine::Engine::new(inv.clone(), crate::engine::EngineLimits::default()).unwrap();
    let query = serde_json::from_value(json!({"kind":"plan","plan":{"op":"lookup","table":{"name":"Sample","mode":"static"},"ids":[1,42,99]}})).unwrap();
    let result = engine.execute(query, inv.control.clone()).await.unwrap();
    assert_eq!(result.row_count(), 1);
    assert_eq!(serde_json::to_value(result.page(0, 10).unwrap()).unwrap()["rows"], json!([[1, 42]]));
    let report = &result.provenance()["lookupRequirements"][0];
    assert_eq!(report["classification"], "exact_visible_integer_id");
    assert_eq!(report["identityDomain"], "public_id_not_physical_locator");
    assert_eq!(report["requestedIds"], json!([1, 42, 99]));
    assert_eq!(report["missingIds"], json!([1, 99]));
    assert_eq!(report["unresolvedIds"], json!([]));
    drop(result);
    let limited = serde_json::from_value(json!({"kind":"plan","plan":{"op":"limit","offset":0,"count":0,"input":{"op":"lookup","table":{"name":"Sample","mode":"static"},"ids":[42,99]}}})).unwrap();
    let result = engine.execute(limited, inv.control.clone()).await.unwrap();
    assert_eq!(result.row_count(), 0);
    let report = &result.provenance()["lookupRequirements"][0];
    assert_eq!(report["missingIds"], json!([]));
    assert_eq!(report["unresolvedIds"], json!([42, 99]));
    drop(result);
    engine.close().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn nonidentity_float_id_lookup_keeps_plan_execution_without_fabricated_missing_report() {
    let mut spec = fixture::table(
        "Sample",
        921,
        &[("RecordID", "uint"), ("ID", "float")],
        &[vec![Cell::U(1), Cell::F(42.0)]],
    );
    spec.dbd = spec
        .dbd
        .replace("RecordID<u32>", "$id$RecordID<u32>")
        .replace("$id$ID<32>", "ID<32>");
    let fx = Fixture::new(&[spec]);
    let inv = fx.invocation("data sql", &[]);
    let mut engine =
        crate::engine::Engine::new(inv.clone(), crate::engine::EngineLimits::default()).unwrap();
    let query = serde_json::from_value(json!({"kind":"plan","plan":{"op":"lookup","table":{"name":"Sample","mode":"static"},"ids":[42]}})).unwrap();
    let result = engine.execute(query, inv.control.clone()).await.unwrap();
    assert_eq!(result.row_count(), 1);
    let report = &result.provenance()["lookupRequirements"][0];
    assert_eq!(report["classification"], "unavailable_non_integer_id");
    assert!(report["missingIds"].is_null());
    assert!(report["unresolvedIds"].is_null());
    drop(result);
    let full = serde_json::from_value(
        json!({"kind":"plan","plan":{"op":"scan","table":{"name":"Sample","mode":"static"}}}),
    )
    .unwrap();
    let result = engine.execute(full, inv.control.clone()).await.unwrap();
    assert_eq!(result.row_count(), 1);
    assert_eq!(
        result.provenance()["sources"][0]["sessionSnapshot"]["reused"],
        false,
        "a public-ID filter did not publish whole physical source coverage"
    );
    assert_eq!(result.provenance()["sources"][0]["sessionSnapshot"]["sourceReadingScope"], "current_request");
    drop(result);
    engine.close().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn replacing_partial_cache_keeps_live_result_owner_and_does_not_accumulate_entries() {
    let rows = (0..4096)
        .map(|i| vec![Cell::U(i * 2 + 1), Cell::I64(7)])
        .collect::<Vec<_>>();
    let fx = Fixture::new(&[fixture::table(
        "Sample",
        921,
        &[("ID", "uint"), ("Value", "signed64")],
        &rows,
    )]);
    cache(&fx);
    let inv = fx.invocation("data sql", &[]);
    let mut engine =
        crate::engine::Engine::new(inv.clone(), crate::engine::EngineLimits::default()).unwrap();
    let query = |ids: Vec<u32>| {
        serde_json::from_value(json!({"kind":"plan","plan":{"op":"lookup","table":{"name":"Sample","mode":"static"},"ids":ids}})).unwrap()
    };
    let first = engine
        .execute(query(vec![1]), inv.control.clone())
        .await
        .unwrap();
    for ids in [vec![1, 4097], vec![2049, 4097], vec![1]] {
        let result = engine
            .execute(query(ids.clone()), inv.control.clone())
            .await
            .unwrap();
        assert_eq!(result.row_count(), ids.len());
        assert_eq!(engine.cached_source_count(), 1);
        drop(result);
        let page = first.page(0, 1).unwrap();
        assert_eq!(page.rows[0][0], json!(1));
    }
    drop(first);
    engine.close().unwrap();
    assert_eq!(engine.cached_source_count(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_capture_contains_complete_rows_and_original_typed_request() {
    use crate::engine::{Engine, EngineLimits, QueryPlan, QueryRequest};
    let rows = (0..1201)
        .map(|id| vec![Cell::U(id), Cell::I64(9_007_199_254_740_993)])
        .collect::<Vec<_>>();
    let table = fixture::table(
        "Sample",
        921,
        &[("ID", "uint"), ("Exact", "signed64")],
        &rows,
    );
    let fx = Fixture::new(&[table]);
    let inv = fx.invocation("data agent serve", &[]);
    let control = inv.control.clone();
    let mut engine = Engine::new(inv, EngineLimits::default()).unwrap();
    let query = QueryRequest::Plan {
        plan: QueryPlan::Scan {
            table: request(SourceMode::Static, vec![]),
        },
    };
    let result = engine
        .execute(query.clone(), control.clone())
        .await
        .unwrap();
    assert_eq!(result.row_count(), 1201);
    let captured = engine.capture(&result, control).unwrap();
    let receipt = serde_json::to_value(&captured).unwrap();
    let stored = evidence::read_capture(&fx.home, receipt["id"].as_str().unwrap()).unwrap();
    assert_eq!(receipt, stored);
    let body: serde_json::Value =
        serde_json::from_slice(&evidence::read_blob(&fx.home, &receipt["blob"], 16 << 20).unwrap())
            .unwrap();
    assert_eq!(body["rows"].as_array().unwrap().len(), 1201);
    assert_eq!(body["rows"][1200][1].as_i64(), Some(9_007_199_254_740_993));
    assert_eq!(
        body["provenance"]["request"],
        serde_json::to_value(query).unwrap()
    );
    assert_eq!(body["complete"], true);
    assert_eq!(receipt["truncated"], false);
    drop(captured);
    assert_eq!(result.page(1200, 1).unwrap().rows.len(), 1);
    drop(result);
    engine.close().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn capture_cancel_and_foreign_session_reject_without_consuming_handle() {
    use crate::engine::{Engine, EngineLimits, QueryPlan, QueryRequest};
    let fx = Fixture::new(&[basic()]);
    let inv = fx.invocation("data agent serve", &[]);
    let control = inv.control.clone();
    let mut engine = Engine::new(inv.clone(), EngineLimits::default()).unwrap();
    let mut other = Engine::new(inv, EngineLimits::default()).unwrap();
    let result = engine
        .execute(
            QueryRequest::Plan {
                plan: QueryPlan::Scan {
                    table: request(SourceMode::Static, vec![]),
                },
            },
            control.clone(),
        )
        .await
        .unwrap();
    let cancelled = lycheedev_core::control::OperationControl::new(
        control.deadline(),
        Arc::new(std::sync::atomic::AtomicBool::new(true)),
    );
    assert!(engine.capture(&result, cancelled).is_err());
    assert_eq!(
        other.capture(&result, control.clone()).unwrap_err().code,
        "records.source_identity"
    );
    let captured = engine.capture(&result, control).unwrap();
    drop(captured);
    assert_eq!(result.page(0, 2).unwrap().rows.len(), 2);
    drop(result);
    engine.close().unwrap();
    other.close().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn retained_result_capture_uses_new_control_after_original_cancel() {
    use crate::engine::{Engine, EngineLimits, QueryPlan, QueryRequest};
    use std::sync::atomic::{AtomicBool, Ordering};
    let fx = Fixture::new(&[basic()]);
    let inv = fx.invocation("data agent serve", &[]);
    let flag = Arc::new(AtomicBool::new(false));
    let original =
        lycheedev_core::control::OperationControl::new(inv.control.deadline(), flag.clone());
    let mut engine = Engine::new(inv, EngineLimits::default()).unwrap();
    let result = engine
        .execute(
            QueryRequest::Plan {
                plan: QueryPlan::Scan {
                    table: request(SourceMode::Static, vec![]),
                },
            },
            original,
        )
        .await
        .unwrap();
    flag.store(true, Ordering::Relaxed);
    let captured = engine
        .capture(
            &result,
            lycheedev_core::control::OperationControl::default(),
        )
        .unwrap();
    drop(captured);
    drop(result);
    engine.close().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn capture_reauthenticates_effective_dependencies_before_publication() {
    use crate::engine::{Engine, EngineLimits, QueryPlan, QueryRequest};
    let (fx, hotfix) = effective_fixture();
    let inv = fx.invocation("data agent serve", &[]);
    let control = inv.control.clone();
    let mut engine = Engine::new(inv, EngineLimits::default()).unwrap();
    let result = engine
        .execute(
            QueryRequest::Plan {
                plan: QueryPlan::Scan {
                    table: request(SourceMode::Effective, vec![hotfix.clone()]),
                },
            },
            control.clone(),
        )
        .await
        .unwrap();
    evidence::remove_capture(&fx.home, &hotfix).unwrap();
    assert!(engine.capture(&result, control).is_err());
    assert!(!result.page(0, 10).unwrap().rows.is_empty());
    drop(result);
    engine.close().unwrap();
}
