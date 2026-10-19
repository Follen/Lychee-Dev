//! Hand-derived public SQL oracles. A decode witness distinguishes execution
//! reuse from cache prepare/readthrough; fixtures are not real-game evidence.
use super::*;
use crate::generation::observations;
use lycheedev_core::{metadata::Metadata, storage, workspace::data_disk::DataCachePaths};
use std::{fs, path::PathBuf, sync::atomic::Ordering};
#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod fixture;
use fixture::{Cell, Fixture};

struct Observe;
impl Observe {
    fn new() -> Self {
        observations::MANIFEST_ADMISSIONS.with(|n| n.set(0));
        observations::ORIGINAL_WDC.with(|n| n.set(false));
        observations::BATCH_READS.with(|n| n.set(0));
        observations::CLOSES.with(|n| n.set(0));
        observations::FAIL_CLOSES.with(|n| n.set(0));
        provider::WDC_ROWS.with(|n| n.set(0));
        Self
    }
}
impl Drop for Observe {
    fn drop(&mut self) {
        observations::ORIGINAL_WDC.with(|n| n.set(false));
        observations::FAIL_CLOSES.with(|n| n.set(0));
    }
}
fn counts() -> (usize, usize, usize) {
    (
        observations::BATCH_READS.with(|n| n.get()),
        observations::CLOSES.with(|n| n.get()),
        provider::WDC_ROWS.with(|n| n.get()),
    )
}
fn prepare(fx: &Fixture, name: &str) -> Outcome {
    let mut inv = fx.invocation("data cache prepare", &[]);
    inv.positional = Some(name.into());
    crate::dispatch(&inv).unwrap()
}
fn object(fx: &Fixture, out: &Outcome) -> PathBuf {
    DataCachePaths::new(&fx.home)
        .object(out.data["object"]["sha256"].as_str().unwrap())
        .unwrap()
}
fn captures(fx: &Fixture) -> Vec<Value> {
    let metadata = Metadata::open(&fx.home, false).unwrap();
    let result = metadata.list("capture/", 1000).unwrap();
    metadata.close().unwrap();
    result
}
fn sample() -> Fixture {
    Fixture::new(&[fixture::table(
        "Sample",
        201,
        &[("ID", "uint"), ("Value", "uint")],
        &[vec![Cell::U(1), Cell::U(42)], vec![Cell::U(2), Cell::U(99)]],
    )])
}
fn verify_capture(fx: &Fixture, out: &Outcome) {
    assert_eq!(out.provenance.len(), 1);
    let cap = &out.provenance[0];
    assert_eq!(
        evidence::read_capture(&fx.home, cap["id"].as_str().unwrap()).unwrap(),
        *cap
    );
    let bytes = evidence::read_blob(&fx.home, &cap["blob"], 16 << 20).unwrap();
    assert_eq!(storage::sha256(&bytes), cap["blob"]["sha256"]);
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), out.data);
}
#[test]
fn sql_generation_point_uses_typed_batch_and_exact_arrays_in_public_capture() {
    let rows: Vec<_> = (1..=1025)
        .rev()
        .map(|id| {
            vec![
                Cell::U(id),
                Cell::I64(9_007_199_254_740_993),
                Cell::Array(vec![Cell::U(id), Cell::U(0), Cell::U(u32::MAX)]),
                Cell::Array(vec![Cell::F(0.1), Cell::F(-0.0)]),
                Cell::Text("中文🙂".into()),
            ]
        })
        .collect();
    let fx = Fixture::new(&[fixture::table(
        "Sample",
        201,
        &[
            ("ID", "uint"),
            ("Exact", "signed64"),
            ("Array", "array3"),
            ("Floats", "array2float"),
            ("Text", "text"),
        ],
        &rows,
    )]);
    let prepared = prepare(&fx, "Sample");
    assert_eq!(prepared.data["batchCount"], 2);
    let _observe = Observe::new();
    let out = crate::dispatch(&fx.invocation(
        "data sql",
        &[(
            "--sql",
            "SELECT ID,Exact,\"Array[2]\",\"Floats[0]\",Text FROM Sample WHERE ID=1025",
        )],
    ))
    .unwrap();
    assert_eq!(
        out.data["result"]["rows"],
        json!([[
            1025,
            9_007_199_254_740_993i64,
            u32::MAX,
            f64::from(0.1f32),
            "中文🙂"
        ]])
    );
    assert_eq!(
        out.data["sources"][0]["typedGenerationCache"]["status"],
        "hit"
    );
    assert_eq!(
        counts(),
        (1, 1, 0),
        "a point cursor decodes one IPC batch, no WDC row"
    );
    verify_capture(&fx, &out);
}
#[test]
fn sql_generation_u64_max_preserves_integer_tokens_and_csv() {
    let mut spec = fixture::table(
        "Sample",
        201,
        &[("ID", "uint"), ("Exact", "signed64")],
        &[vec![Cell::U(7), Cell::I64(-1)]],
    );
    spec.dbd = spec.dbd.replace("Exact<64>", "Exact<u64>");
    let fx = Fixture::new(&[spec]);
    prepare(&fx, "Sample");
    let _observe = Observe::new();
    let out = crate::dispatch(&fx.invocation(
        "data sql",
        &[("--sql", "SELECT Exact FROM Sample WHERE ID=7")],
    ))
    .unwrap();
    assert_eq!(out.data["result"]["rows"][0][0].as_u64(), Some(u64::MAX));
    verify_capture(&fx, &out);
    let path = fx.root.join("exact.csv");
    let out = crate::dispatch(&fx.invocation(
        "data sql",
        &[
            ("--sql", "SELECT Exact FROM Sample WHERE ID=7"),
            ("--encoding", "csv"),
            ("--output", path.to_str().unwrap()),
        ],
    ))
    .unwrap();
    assert_eq!(out.provenance.len(), 2);
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("18446744073709551615")
    );
    assert_eq!(counts(), (2, 2, 0));
}
#[test]
fn sql_generation_scan_projection_filter_aggregate_and_self_join_have_independent_cursors() {
    let fx = sample();
    prepare(&fx, "Sample");
    let _observe = Observe::new();
    for (sql, expected) in [
        (
            "SELECT Value FROM Sample WHERE Value>50 ORDER BY Value DESC LIMIT 1",
            json!([[99]]),
        ),
        ("SELECT SUM(Value),COUNT(*) FROM Sample", json!([[141, 2]])),
        (
            "SELECT a.ID,b.Value FROM Sample a JOIN Sample b ON a.ID=b.ID ORDER BY a.ID",
            json!([[1, 42], [2, 99]]),
        ),
        (
            "SELECT a.ID,(SELECT b.Value FROM Sample b WHERE b.ID=a.ID) FROM Sample a ORDER BY a.ID",
            json!([[1, 42], [2, 99]]),
        ),
    ] {
        let out = crate::dispatch(&fx.invocation("data sql", &[("--sql", sql)])).unwrap();
        assert_eq!(out.data["result"]["rows"], expected, "{sql}");
    }
    assert!(counts().0 >= 4);
    assert_eq!((counts().1, counts().2), (4, 0));
}
#[test]
fn sql_generation_absent_does_not_create_a_generation_and_uses_original_rows() {
    let fx = sample();
    let _observe = Observe::new();
    let out = crate::dispatch(&fx.invocation(
        "data sql",
        &[("--sql", "SELECT Value FROM Sample WHERE ID=1")],
    ))
    .unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[42]]));
    assert!(out.data["sources"][0].get("typedGenerationCache").is_none());
    assert_eq!(counts(), (0, 0, 1));
    assert_eq!(
        fs::read_dir(DataCachePaths::new(&fx.home).generations)
            .unwrap()
            .count(),
        0
    );
}
#[test]
fn sql_generation_missing_body_under_reduced_quota_is_explicit_zero_write_wdc_fallback() {
    let fx = sample();
    let prepared = prepare(&fx, "Sample");
    let body = object(&fx, &prepared);
    fs::remove_file(&body).unwrap();
    let paths = DataCachePaths::new(&fx.home);
    let manifest = paths.generations.join(format!(
        "{}.json",
        prepared.data["generation"].as_str().unwrap()
    ));
    let before = fs::read(&manifest).unwrap();
    fs::write(fx.home.join("config.json"),br#"{"schema":"lycheedev.config.v1","cache":{"maxBytes":21474836480},"dataCache":{"maxBytes":1}}"#).unwrap();
    let _observe = Observe::new();
    let out = crate::dispatch(&fx.invocation(
        "data sql",
        &[("--sql", "SELECT Value FROM Sample WHERE ID=1")],
    ))
    .unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[42]]));
    let cache = &out.data["sources"][0]["typedGenerationCache"];
    assert_eq!(cache["status"], "unavailable");
    assert_eq!(cache["reused"], false);
    assert_eq!(cache["reason"], "materialized_body_missing");
    assert_eq!(cache["publicationThisCommand"], false);
    assert!(!body.exists());
    assert_eq!(fs::read(&manifest).unwrap(), before);
    assert_eq!(fs::read_dir(paths.staging).unwrap().count(), 0);
    assert_eq!(counts(), (0, 0, 1));
    verify_capture(&fx, &out);
}
#[test]
fn sql_generation_corrupt_body_is_hard_error_without_wdc_fallback_or_capture() {
    let fx = sample();
    let prepared = prepare(&fx, "Sample");
    let path = object(&fx, &prepared);
    let mut bytes = fs::read(&path).unwrap();
    bytes[12] ^= 1;
    fs::write(&path, bytes).unwrap();
    let before = captures(&fx).len();
    let _observe = Observe::new();
    assert!(
        crate::dispatch(&fx.invocation("data sql", &[("--sql", "SELECT * FROM Sample")])).is_err()
    );
    assert_eq!(counts(), (0, 0, 0));
    assert_eq!(captures(&fx).len(), before);
}
#[test]
fn sql_generation_partial_source_retains_coverage_and_incomplete_capture() {
    let spec = fixture::encrypted_sections(201);
    let (_, encoded) = fixture::encrypted_frame(&spec.raw);
    let fx = Fixture::with_encoded_table(spec, encoded);
    let prepared = prepare(&fx, "Sample");
    assert!(!prepared.complete);
    let _observe = Observe::new();
    let out =
        crate::dispatch(&fx.invocation("data sql", &[("--sql", "SELECT ID,Value FROM Sample")]))
            .unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[1, 42]]));
    assert!(!out.complete);
    assert_eq!(
        out.data["sources"][0]["typedGenerationCache"]["status"],
        "hit"
    );
    assert_eq!(out.provenance[0]["complete"], false);
    assert_eq!(counts(), (1, 1, 0));
    verify_capture(&fx, &out);
}
#[test]
fn sql_generation_source_recheck_and_cancellation_close_reader_before_return() {
    let fx = sample();
    prepare(&fx, "Sample");
    let inv = fx.invocation("data sql", &[("--sql", "SELECT * FROM Sample")]);
    let before = captures(&fx).len();
    let catalog = fx.client.parent().unwrap().join(".build.info");
    let original = fs::read(&catalog).unwrap();
    let _observe = Observe::new();
    let error = sql_inner_after_execute(&inv, &mut || {
        let mut changed = original.clone();
        changed.push(b'\n');
        fs::write(&catalog, changed)?;
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.code, "records.pinned_build_changed");
    assert_eq!(counts(), (1, 1, 0));
    assert_eq!(captures(&fx).len(), before);
    fs::write(&catalog, original).unwrap();
    let inv = fx.invocation("data sql", &[("--sql", "SELECT * FROM Sample")]);
    let stop = inv.control.cancel_handle();
    let error = sql_inner_after_execute(&inv, &mut || {
        stop.store(true, Ordering::Release);
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(counts(), (2, 2, 0));
    assert_eq!(captures(&fx).len(), before);
}
#[test]
fn sql_generation_two_tables_close_all_after_first_close_error_and_preserve_committed_cap() {
    let fx = Fixture::new(&[
        fixture::table("First", 201, &[("ID", "uint")], &[vec![Cell::U(1)]]),
        fixture::table("Second", 202, &[("ID", "uint")], &[vec![Cell::U(1)]]),
    ]);
    let one = prepare(&fx, "First");
    let two = prepare(&fx, "Second");
    for sql in [
        "SELECT a.ID FROM First a JOIN Second b ON a.ID=b.ID",
        "SELECT b.ID FROM Second b JOIN First a ON a.ID=b.ID",
    ] {
        let _observe = Observe::new();
        observations::FAIL_CLOSES.with(|n| n.set(2));
        let before = captures(&fx).len();
        let error = crate::dispatch(&fx.invocation("data sql", &[("--sql", sql)])).unwrap_err();
        assert_eq!(error.code, "storage.close_failed");
        assert_eq!(counts().1, 2);
        assert_eq!(counts().2, 0);
        assert_eq!(
            error.details["schema"],
            "lycheedev.query-committed-error.v1"
        );
        assert_eq!(
            error.details["completedCaptures"].as_array().unwrap().len(),
            1
        );
        assert_eq!(captures(&fx).len(), before + 1);
        assert_eq!(
            error.details["primary"]["details"]["causes"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        // Existing Windows deny-delete ownership must have ended for both files.
        for prepared in [&one, &two] {
            let path = object(&fx, prepared);
            let moved = path.with_extension("closed");
            fs::rename(&path, &moved).unwrap();
            fs::rename(&moved, &path).unwrap();
        }
    }
}
#[test]
fn sql_generation_refundable_owners_end_after_success_and_budget_rejection() {
    let fx = sample();
    prepare(&fx, "Sample");
    for work in [100_000_000, 1] {
        let inv = fx.invocation("data sql", &[("--sql", "SELECT * FROM Sample")]);
        let budget = Arc::new(Budget::controlled(
            Cost {
                decode_work: work,
                ..Cost::defaults()
            },
            inv.control.deadline(),
            inv.control.clone(),
        ));
        let result = sql_inner_budgeted(&inv, &budget, &mut || Ok(()));
        if work == 1 {
            assert!(result.is_err());
        } else {
            assert!(result.is_ok());
        }
        assert_eq!(budget.refundable_retained_bytes(), 0);
    }
}

#[test]
fn sql_generation_miss_preserves_original_wdc_tight_fixed_budget() {
    // Distinct equivalent fresh workspaces keep warm source/metadata state equal.
    // The bypass is test-only, never a user-selectable fallback engine.
    for original in [true, false] {
        let fx = sample();
        let _observe = Observe::new();
        observations::ORIGINAL_WDC.with(|flag| flag.set(original));
        let inv = fx.invocation(
            "data sql",
            &[("--sql", "SELECT Value FROM Sample WHERE ID=1")],
        );
        let budget = Arc::new(Budget::controlled(
            Cost {
                retained_bytes: 200 << 20,
                metadata_bytes: 80 << 20,
                ..Cost::defaults()
            },
            inv.control.deadline(),
            inv.control.clone(),
        ));
        let out = sql_inner_budgeted(&inv, &budget, &mut || Ok(()))
            .expect("same fixed cap must admit original and missing-generation WDC SQL");
        assert_eq!(out.data["result"]["rows"], json!([[42]]));
        assert_eq!(out.data["memoryBudget"]["growths"], 0);
        assert_eq!(budget.refundable_retained_bytes(), 0);
        assert!(out.data["sources"][0].get("typedGenerationCache").is_none());
        assert_eq!(counts(), (0, 0, 1));
        assert_eq!(
            observations::MANIFEST_ADMISSIONS.with(|n| n.get()),
            0,
            "a missing manifest never reserves raw/parse scratch"
        );
        assert_eq!(
            fs::read_dir(DataCachePaths::new(&fx.home).generations)
                .unwrap()
                .count(),
            0
        );
    }
}

#[test]
fn sql_generation_static_and_effective_share_source_but_preserve_overlay_in_both_orders() {
    let spec = fixture::table(
        "Sample",
        201,
        &[("ID", "uint"), ("Value", "uint")],
        &[
            vec![Cell::U(1), Cell::U(42)],
            vec![Cell::U(2), Cell::U(99)],
            vec![Cell::U(3), Cell::U(66)],
        ],
    );
    let hash = spec.hash;
    let fx = Fixture::new(&[spec]);
    prepare(&fx, "Sample");
    let mut raw = b"XFTH".to_vec();
    raw.extend(9u32.to_le_bytes());
    raw.extend(69933u32.to_le_bytes());
    raw.resize(44, 0);
    for (id, status, payload) in [
        (
            1u32,
            1u8,
            [1u32.to_le_bytes(), 100u32.to_le_bytes()].concat(),
        ),
        (2, 2, vec![]),
    ] {
        let mut header = vec![0; 32];
        header[..4].copy_from_slice(b"XFTH");
        for (at, value) in [(8, 1u32), (16, hash), (20, id), (24, payload.len() as u32)] {
            header[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        header[28] = status;
        raw.extend(header);
        raw.extend(payload);
    }
    let cap=evidence::commit_capture(&fx.home,&raw,"application/octet-stream",json!({"kind":"hotfix-cache","locator":"fixture:typed-effective","snapshot":fx.pin,"dataBuild":fixture::BUILD}),true,false).unwrap();
    for (sql, expected) in [
        (
            "SELECT s.ID,s.Value,e.Value FROM Sample s LEFT JOIN effective.Sample e ON s.ID=e.ID ORDER BY s.ID",
            json!([[1, 42, 100], [2, 99, null], [3, 66, 66]]),
        ),
        (
            "SELECT e.ID,e.Value,s.Value FROM effective.Sample e JOIN Sample s ON e.ID=s.ID ORDER BY e.ID",
            json!([[1, 100, 42], [3, 66, 66]]),
        ),
    ] {
        let _observe = Observe::new();
        let file = fx.root.join("typed-effective.json");
        fs::write(
            &file,
            serde_json::to_vec(&json!({"sql":sql,"hotfix":[cap["id"]]})).unwrap(),
        )
        .unwrap();
        let out =
            crate::dispatch(&fx.invocation("data sql", &[("--file", file.to_str().unwrap())]))
                .unwrap();
        assert_eq!(out.data["result"]["rows"], expected);
        assert_eq!(counts().1, 1);
        assert!(counts().0 >= 1);
        assert!(
            counts().2 >= 1,
            "effective rows remain on the original overlay decoder"
        );
        assert_eq!(
            out.data["sources"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|source| source["typedGenerationCache"]["status"] == "hit")
                .count(),
            1
        );
        assert_eq!(out.data["effectiveSources"][0]["deleted"], 1);
    }
}

#[test]
fn sql_generation_oversize_typed_schema_keeps_legal_wdc_projection_and_prepare_strict() {
    let names: Vec<String> = std::iter::once("ID".to_owned())
        .chain((0..63).map(|index| format!("F{index}_{}", "x".repeat(16_700))))
        .collect();
    assert!(names.iter().map(String::len).sum::<usize>() > 1 << 20);
    let fields: Vec<_> = names.iter().map(|name| (name.as_str(), "uint")).collect();
    let values = vec![Cell::U(7); fields.len()];
    for original in [true, false] {
        let spec = fixture::table("Sample", 201, &fields, &[values.clone()]);
        assert!(spec.dbd.len() < 4 << 20);
        assert!(spec.dbd.lines().all(|line| line.len() < 64 << 10));
        let fx = Fixture::new(&[spec]);
        let _observe = Observe::new();
        observations::ORIGINAL_WDC.with(|flag| flag.set(original));
        let inv = fx.invocation("data sql", &[("--sql", "SELECT ID FROM Sample")]);
        let budget = Arc::new(Budget::controlled(
            Cost::defaults(),
            inv.control.deadline(),
            inv.control.clone(),
        ));
        let out = sql_inner_budgeted(&inv, &budget, &mut || Ok(())).unwrap();
        assert_eq!(out.data["result"]["rows"], json!([[7]]));
        assert_eq!(counts(), (0, 0, 1));
        assert_eq!(observations::MANIFEST_ADMISSIONS.with(|n| n.get()), 0);
        assert_eq!(budget.refundable_retained_bytes(), 0);
        if !original {
            let cache = &out.data["sources"][0]["typedGenerationCache"];
            assert_eq!(cache["status"], "unavailable");
            assert_eq!(cache["reason"], "schema_representation_limit");
            assert_eq!(cache["reused"], false);
            assert_eq!(cache["publicationThisCommand"], false);
        }
        verify_capture(&fx, &out);
        let mut inv = fx.invocation("data cache prepare", &[]);
        inv.positional = Some("Sample".into());
        assert_eq!(
            crate::dispatch(&inv).unwrap_err().code,
            "records.query_resource_budget"
        );
        assert_eq!(
            fs::read_dir(DataCachePaths::new(&fx.home).generations)
                .unwrap()
                .count(),
            0
        );
    }
}
