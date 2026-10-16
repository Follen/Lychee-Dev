//! Actual local CASC/DBD/WDC and capture consumers; synthetic fixture, not game evidence.
use super::*;
#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod catalog_fixture;
use catalog_fixture::{Cell::U, Fixture, table};
use lycheedev_core::{
    control::{OperationControl, TrackedCancellation},
    metadata::Metadata,
    storage,
    workspace::{
        self,
        data_resources::{self, Policy},
    },
};
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};
fn fixture() -> Fixture {
    Fixture::new(&[table("Sample", 201, &[("ID", "uint")], &[vec![U(7)]])])
}
fn captures(fx: &Fixture) -> usize {
    let m = Metadata::open(&fx.home, false).unwrap();
    let n = m.list::<Value>("capture/", 100).unwrap().len();
    m.close().unwrap();
    n
}
fn released(fx: &Fixture) {
    data_resources::acquire(&fx.home, Policy::Measurement, &OperationControl::default())
        .unwrap()
        .close()
        .unwrap();
}
fn held(home: &Path) {
    let short = OperationControl::uncancelable(Instant::now() + Duration::from_millis(30));
    assert_eq!(
        data_resources::acquire(home, Policy::Measurement, &short)
            .err()
            .unwrap()
            .code,
        "command.deadline"
    );
}
fn report(value: &Value) {
    let m = &value["memoryBudget"];
    assert_eq!(
        m["mode"],
        if cfg!(windows) {
            "windows_adaptive"
        } else {
            "fixed"
        }
    );
    for key in [
        "retainedBytes",
        "metadataBytes",
        "peakScratchBytes",
        "growths",
    ] {
        assert!(m[key].is_u64(), "{key}");
    }
    assert!(value["resources"]["retainedBytes"].is_u64());
}
fn verify_capture(fx: &Fixture, out: &Outcome) {
    assert_eq!(out.provenance.len(), 1);
    let raw = evidence::read_blob(&fx.home, &out.provenance[0]["blob"], 16 << 20).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&raw).unwrap(), out.data);
    assert_eq!(storage::sha256(&raw), out.provenance[0]["blob"]["sha256"]);
}
fn db2_inv(fx: &Fixture, command: &str, flags: &[(&str, &str)]) -> Invocation {
    let mut inv = fx.invocation(command, flags);
    if command == "data db2" {
        inv.flags.insert("--table".into(), "Sample".into());
    } else {
        inv.positional = Some("Sample".into());
    }
    inv
}
#[test]
fn default_local_sql_reports_memory_at_exact_captured_query_boundary() {
    let fx = fixture();
    let inv = fx.invocation("data sql", &[("--sql", "SELECT ID FROM Sample")]);
    let out = sql(&inv).unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[7]]));
    report(&out.data);
    assert!(out.data["resources"]["retainedBytes"].as_u64().unwrap() >= 64 << 20);
    assert!(out.data["sources"][0].get("memoryBudget").is_none());
    verify_capture(&fx, &out);
    released(&fx);
    fs::remove_dir_all(fx.root).unwrap();
}
#[test]
fn actual_local_sql_samples_only_while_admitted_and_reuses_one_budget_through_growth() {
    let fx = fixture();
    let mut config = workspace::default_config();
    config["download"]["workers"] = json!(1);
    storage::atomic_json(&fx.home.join("config.json"), &config).unwrap();
    let inv = fx.invocation("data sql", &[("--sql", "SELECT ID FROM Sample")]);
    let calls = Arc::new(AtomicU64::new(0));
    let deadline = inv.control.deadline();
    let budget = Arc::new(Budget::query_with_probe(deadline, inv.control.clone(), {
        let calls = calls.clone();
        let home = fx.home.clone();
        Arc::new(move || {
            held(&home);
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(4 << 30)
        })
    }));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let mut executed = false;
    let out = sql_inner_budgeted(&inv, &budget, &mut || {
        executed = true;
        held(&fx.home);
        assert_eq!(budget.memory_snapshot()?.concurrent_queries, 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // Cross the prior allowance after executing real SQL. No physical allocation is fabricated.
        budget.charge_split(Cost::retained(520 << 20), Cost::default())?;
        Ok(())
    })
    .unwrap();
    assert!(executed);
    assert_eq!(out.data["result"]["rows"], json!([[7]]));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(budget.deadline, deadline);
    assert_eq!(
        out.data["memoryBudget"],
        serde_json::to_value(budget.memory_snapshot().unwrap()).unwrap()
    );
    assert_eq!(
        out.data["resources"],
        serde_json::to_value(budget.logical_snapshot().unwrap()).unwrap()
    );
    assert_eq!(out.data["memoryBudget"]["growths"], 1);
    assert!(out.data["memoryBudget"]["retainedBytes"].as_u64().unwrap() > 512 << 20);
    verify_capture(&fx, &out);
    released(&fx);
    fs::remove_dir_all(fx.root).unwrap();
}
#[test]
fn actual_local_sql_explicit_fixed_budget_is_not_silently_replaced() {
    let fx = fixture();
    let inv = fx.invocation("data sql", &[("--sql", "SELECT ID FROM Sample")]);
    // Go ranges.copyPart reserves 2*64MiB chunk scratch in addition to the
    // existing 64MiB SQL executor reservation, even for this small fixture.
    // Its session Root lookup reserves 4096*256 bytes, not the generic parser's
    // 1<<20 match ceiling. This explicit 256MiB cap must therefore suffice.
    let limits = Cost {
        retained_bytes: 256 << 20,
        metadata_bytes: 80 << 20,
        ..Cost::defaults()
    };
    let budget = Arc::new(Budget::controlled(
        limits,
        inv.control.deadline(),
        inv.control.clone(),
    ));
    let out = sql_inner_budgeted(&inv, &budget, &mut || Ok(())).unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[7]]));
    let memory = budget.memory_snapshot().unwrap();
    assert!(memory.peak_scratch_bytes >= 128 << 20);
    assert!(memory.peak_scratch_bytes < 192 << 20);
    assert_eq!(budget.deadline, inv.control.deadline());
    assert_eq!(
        budget.remaining_retained().unwrap(),
        limits.retained_bytes - budget.logical_snapshot().unwrap().retained_bytes,
    );
    assert_eq!(
        out.data["memoryBudget"],
        json!({"mode":"fixed","retainedBytes":256<<20,"metadataBytes":80<<20,
        "peakScratchBytes":budget.memory_snapshot().unwrap().peak_scratch_bytes,"growths":0})
    );
    verify_capture(&fx, &out);
    released(&fx);
    fs::remove_dir_all(fx.root).unwrap();
}
#[test]
fn actual_local_sql_independent_budget_stop_survives_cleanup_without_capture() {
    let fx = fixture();
    let inv = fx.invocation("data sql", &[("--sql", "SELECT ID FROM Sample")]);
    let token = TrackedCancellation::new();
    let independent = OperationControl::tracked(inv.control.deadline(), token.clone());
    let budget = Arc::new(Budget::controlled(
        Cost::defaults(),
        independent.deadline(),
        independent,
    ));
    let before = captures(&fx);
    let mut executed = false;
    let error = sql_inner_budgeted(&inv, &budget, &mut || {
        executed = true;
        held(&fx.home);
        token.cancel();
        Ok(())
    })
    .unwrap_err();
    assert!(executed);
    let expected = budget.check().unwrap_err();
    assert_eq!(
        (error.code, error.exit_code, error.message, error.details),
        (
            expected.code,
            expected.exit_code,
            expected.message,
            expected.details
        )
    );
    inv.control.check().unwrap();
    assert_eq!(captures(&fx), before);
    released(&fx);
    fs::remove_dir_all(fx.root).unwrap();
}
#[test]
fn actual_local_sql_missing_pin_precedes_admission_and_never_samples() {
    let fx = fixture();
    let holder =
        data_resources::acquire(&fx.home, Policy::Measurement, &OperationControl::default())
            .unwrap();
    let mut inv = fx.invocation("data sql", &[("--sql", "SELECT ID FROM Sample")]);
    inv.flags
        .insert("--snapshot".into(), format!("PIN-{}", "0".repeat(64)));
    let calls = Arc::new(AtomicU64::new(0));
    let budget = Arc::new(Budget::query_with_probe(
        inv.control.deadline(),
        inv.control.clone(),
        {
            let calls = calls.clone();
            Arc::new(move || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(4 << 30)
            })
        },
    ));
    assert_eq!(
        sql_inner_budgeted(&inv, &budget, &mut || panic!("must not execute"))
            .unwrap_err()
            .code,
        "vault.record_missing"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(budget.memory_snapshot().is_err());
    holder.close().unwrap();
    released(&fx);
    fs::remove_dir_all(fx.root).unwrap();
}
#[test]
fn actual_local_sql_failed_activation_releases_owner_before_any_source_or_capture() {
    let fx = fixture();
    let inv = fx.invocation("data sql", &[("--sql", "SELECT ID FROM MissingDefinition")]);
    let before = captures(&fx);
    let budget = Arc::new(Budget::query_with_probe(
        inv.control.deadline(),
        inv.control.clone(),
        Arc::new(|| Err(fault("fixture.memory_status", 5, "unavailable"))),
    ));
    let error = sql_inner_budgeted(&inv, &budget, &mut || panic!("must not execute")).unwrap_err();
    assert_eq!(error.code, "records.query_resource_budget");
    assert_eq!(error.details["cause"]["code"], "fixture.memory_status");
    assert_eq!(captures(&fx), before);
    assert!(budget.memory_snapshot().is_err());
    released(&fx);
    fs::remove_dir_all(fx.root).unwrap();
}
#[test]
fn default_local_db2_memory_is_only_on_data_context_profiles() {
    let fx = fixture();
    for (command, flags) in [
        ("data db2", vec![("--id", "7")]),
        ("data db2", vec![("--limit", "1")]),
        ("data db2 schema", vec![]),
        (
            "data db2 search",
            vec![("--field", "ID"), ("--query", "7"), ("--limit", "1")],
        ),
        (
            "data db2 foreign-key",
            vec![("--field", "ID"), ("--value", "7"), ("--limit", "1")],
        ),
    ] {
        let out = crate::dispatch(&db2_inv(&fx, command, &flags)).unwrap();
        if command == "data db2" {
            assert!(out.data.get("memoryBudget").is_none());
        } else {
            report(&out.data);
        }
        verify_capture(&fx, &out);
        released(&fx);
    }
    fs::remove_dir_all(fx.root).unwrap();
}
#[test]
fn default_local_record_and_navigator_preserve_opposite_pin_admission_order() {
    let fx = fixture();
    let holder =
        data_resources::acquire(&fx.home, Policy::Measurement, &OperationControl::default())
            .unwrap();
    let mut record = db2_inv(&fx, "data db2", &[("--id", "7")]);
    record
        .flags
        .insert("--snapshot".into(), format!("PIN-{}", "0".repeat(64)));
    assert_eq!(
        crate::dispatch(&record).unwrap_err().code,
        "vault.record_missing"
    );
    let mut schema = db2_inv(&fx, "data db2 schema", &[]);
    schema
        .flags
        .insert("--snapshot".into(), format!("PIN-{}", "0".repeat(64)));
    schema.control = OperationControl::uncancelable(Instant::now() + Duration::from_millis(50));
    assert_eq!(
        crate::dispatch(&schema).unwrap_err().code,
        "command.deadline"
    );
    holder.close().unwrap();
    schema.control = OperationControl::default();
    assert_eq!(
        crate::dispatch(&schema).unwrap_err().code,
        "vault.record_missing"
    );
    assert_eq!(captures(&fx), 0);
    released(&fx);
    fs::remove_dir_all(fx.root).unwrap();
}
#[test]
fn default_local_stream_reports_summary_without_changing_wire_or_manifest() {
    let fx = fixture();
    let inv = db2_inv(
        &fx,
        "data db2 stream",
        &[("--format", "jsonl"), ("--limit", "1")],
    );
    let mut frames = Vec::new();
    let out = crate::stream::stream(&inv, &mut |frame| {
        if frame["frame"] == "record" {
            held(&fx.home);
        }
        if frame["frame"] == "end" {
            released(&fx);
        }
        frames.push(frame.clone());
        Ok(())
    })
    .unwrap();
    report(&out.data);
    assert_eq!(
        frames
            .iter()
            .map(|f| f["frame"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["begin", "record", "end"]
    );
    for frame in &frames {
        assert!(frame.get("memoryBudget").is_none());
        assert!(frame["begin"].get("memoryBudget").is_none());
        assert!(frame["end"].get("memoryBudget").is_none());
    }
    let raw = evidence::read_blob(&fx.home, &out.provenance[0]["blob"], 16 << 20).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&raw).unwrap(),
        json!({"begin":frames[0]["begin"],"end":frames[2]["end"]})
    );
    released(&fx);
    fs::remove_dir_all(fx.root).unwrap();
}
#[test]
fn default_local_navigation_and_asset_use_admission_with_go_report_shapes() {
    let fx = Fixture::new(&catalog_fixture::items());
    let inv = fx.invocation("data item get", &[("--item-id", "7")]);
    let mut visited = false;
    let out = crate::navigate::dispatch_after_read(&inv, &mut || {
        visited = true;
        held(&fx.home);
        Ok(())
    })
    .unwrap();
    assert!(visited);
    report(&out.data);
    assert_eq!(out.data["tables"].as_array().unwrap().len(), 2);
    verify_capture(&fx, &out);
    released(&fx);
    let id = out.data["tables"][0]["fileDataID"]
        .as_u64()
        .unwrap()
        .to_string();
    let asset = asset(&fx.invocation("asset inspect", &[("--file-id", &id)])).unwrap();
    assert!(asset.data.get("memoryBudget").is_none());
    assert_eq!(asset.data["contentVerified"], true);
    assert_eq!(asset.provenance.len(), 1);
    released(&fx);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn default_local_read_files_waits_for_real_owner_and_releases_after_return() {
    let fx = fixture();
    let mut inv = fx.invocation("data sql", &[]);
    let pin = selection::resolve_pinned(&inv)
        .unwrap()
        .data
        .clone()
        .unwrap();
    let holder =
        data_resources::acquire(&fx.home, Policy::Measurement, &OperationControl::default())
            .unwrap();
    inv.control = OperationControl::uncancelable(Instant::now() + Duration::from_millis(50));
    let error = read_files(&inv, &pin, &[201], inv.control.deadline())
        .err()
        .unwrap();
    assert_eq!(error.code, "command.deadline");
    holder.close().unwrap();
    inv.control = OperationControl::default();
    let files = read_files(&inv, &pin, &[201], inv.control.deadline()).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(&files[0].bytes[..4], b"WDC3");
    assert_eq!(files[0].reading["contentVerified"], true);
    assert!(files[0].reading.get("memoryBudget").is_none());
    drop(files);
    released(&fx);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn default_local_reader_request_errors_precede_busy_slots_but_pin_identity_follows() {
    let fx = fixture();
    let original = fx.invocation("data sql", &[]);
    let pin = selection::resolve_pinned(&original)
        .unwrap()
        .data
        .clone()
        .unwrap();
    let holder =
        data_resources::acquire(&fx.home, Policy::Measurement, &OperationControl::default())
            .unwrap();
    for (flags, ids, expected) in [
        (
            vec![("--content-variant", "bad")],
            vec![0],
            "records.file_query",
        ),
        (vec![], vec![0], "records.file_query"),
        (vec![], vec![], "records.file_query"),
        (vec![], vec![201; 33], "records.file_query"),
        (
            vec![("--installation", "")],
            vec![201],
            "records.source_unavailable",
        ),
        (
            vec![("--cdn", "true")],
            vec![201],
            "records.source_unavailable",
        ),
        (
            vec![("--max-bytes", "0")],
            vec![201],
            "command.invalid_arguments",
        ),
        (
            vec![("--max-bytes", "invalid")],
            vec![201],
            "command.invalid_arguments",
        ),
    ] {
        let mut inv = fx.invocation("data sql", &flags);
        inv.control = OperationControl::uncancelable(Instant::now() + Duration::from_millis(200));
        let mut malformed = pin.clone();
        malformed.full_build = "invalid".into();
        let error = read_files(&inv, &malformed, &ids, inv.control.deadline())
            .err()
            .unwrap();
        assert_eq!(error.code, expected);
        inv.control.check().unwrap();
    }
    let mut missing = fx.invocation("data sql", &[]);
    missing.flags.remove("--installation");
    assert_eq!(
        read_files(&missing, &pin, &[201], missing.control.deadline())
            .err()
            .unwrap()
            .code,
        "records.source_unavailable"
    );
    let mut malformed = pin.clone();
    malformed.full_build = "invalid".into();
    let mut waiting = fx.invocation("data sql", &[]);
    waiting.control = OperationControl::uncancelable(Instant::now() + Duration::from_millis(50));
    assert_eq!(
        read_files(&waiting, &malformed, &[201], waiting.control.deadline())
            .err()
            .unwrap()
            .code,
        "command.deadline"
    );
    holder.close().unwrap();
    waiting.control = OperationControl::default();
    assert_eq!(
        read_files(&waiting, &malformed, &[201], waiting.control.deadline())
            .err()
            .unwrap()
            .code,
        "selection.unresolved_data"
    );
    released(&fx);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn default_local_reader_original_stop_and_independent_deadline_precede_invalid_request() {
    let fx = fixture();
    let mut inv = fx.invocation("data sql", &[("--content-variant", "bad")]);
    let mut pin = selection::resolve_pinned(&inv)
        .unwrap()
        .data
        .clone()
        .unwrap();
    pin.full_build = "invalid".into();
    inv.control.cancel_handle().store(true, Ordering::Release);
    assert_eq!(
        read_files(&inv, &pin, &[0], inv.control.deadline())
            .err()
            .unwrap()
            .code,
        "command.cancelled"
    );
    inv.control = OperationControl::default();
    assert_eq!(
        read_files(&inv, &pin, &[0], Instant::now())
            .err()
            .unwrap()
            .code,
        "records.query_resource_budget"
    );
    inv.control.check().unwrap();
    released(&fx);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn actual_local_asset_capture_id_survives_late_close_with_cli_marker_contract() {
    let fx = fixture();
    let inv = fx.invocation("asset inspect", &[("--file-id", "201")]);
    let primary = fault("records.metadata_limit", 3, "checked close failed")
        .with_details(json!({"schema":"fixture.close.v1","cause":"closed after actual capture"}));
    let mut observed = false;
    let error = asset_after_finish(&inv, &mut |result| {
        let out = result?;
        assert_eq!(out.provenance.len(), 1);
        observed = true;
        released(&fx);
        Err(primary.clone())
    })
    .unwrap_err();
    assert!(observed);
    assert_eq!(captures(&fx), 1);
    assert_eq!(
        error.details["schema"],
        "lycheedev.asset-committed-error.v1"
    );
    assert_eq!(error.details["outputPublished"], false);
    assert_eq!(
        error.details["primary"],
        serde_json::to_value(&primary).unwrap()
    );
    let ids = error.details["completedCaptures"].as_array().unwrap();
    assert_eq!(ids.len(), 1);
    let id = ids[0].as_str().unwrap();
    assert!(id.starts_with("CAP-"));
    assert_eq!(id.len(), 68);
    let capture = evidence::read_capture(&fx.home, id).unwrap();
    assert_eq!(capture["provenance"]["kind"], "asset");
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        (primary.code.as_str(), primary.exit_code)
    );
    assert_eq!(
        error.message,
        format!(
            "{}\nasset captures already committed: {id}",
            primary.message
        )
    );
    released(&fx);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn actual_local_sql_fixed_budget_below_decoder_scratch_fails_without_growth_or_capture() {
    let fx = fixture();
    let inv = fx.invocation("data sql", &[("--sql", "SELECT ID FROM Sample")]);
    let budget = Arc::new(Budget::controlled(
        Cost {
            retained_bytes: 96 << 20,
            metadata_bytes: 80 << 20,
            ..Cost::defaults()
        },
        inv.control.deadline(),
        inv.control.clone(),
    ));
    let before = captures(&fx);
    let error = sql_inner_budgeted(&inv, &budget, &mut || Ok(())).unwrap_err();
    assert_eq!(error.code, "records.query_resource_budget");
    assert!(error.message.contains("scratch_bytes"), "{error:?}");
    let memory = serde_json::to_value(budget.memory_snapshot().unwrap()).unwrap();
    assert_eq!(memory["mode"], "fixed");
    assert_eq!(memory["retainedBytes"], 96 << 20);
    assert_eq!(memory["growths"], 0);
    assert!(memory["peakScratchBytes"].as_u64().unwrap() < 128 << 20);
    assert_eq!(
        budget.remaining_retained().unwrap(),
        (96 << 20) - budget.logical_snapshot().unwrap().retained_bytes,
    );
    assert_eq!(captures(&fx), before);
    released(&fx);
    fs::remove_dir_all(fx.root).unwrap();
}
