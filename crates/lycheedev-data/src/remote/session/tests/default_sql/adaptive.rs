//! Real default CDN queries over authenticated offline inputs; no HTTP is mocked in the consumer.
use super::*;
use lycheedev_core::{control::TrackedCancellation, workspace::data_resources};
fn report(data: &Value) {
    let memory = &data["memoryBudget"];
    assert_eq!(
        memory["mode"],
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
        assert!(memory[key].is_u64(), "{key}");
    }
    assert!(data["resources"]["retainedBytes"].is_u64());
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
fn released(f: &SqlFixture) {
    data_resources::acquire(
        &f.source.home,
        Policy::Measurement,
        &OperationControl::default(),
    )
    .unwrap()
    .close()
    .unwrap();
}
fn db2(f: &SqlFixture, command: &str, flags: &[(&str, &str)]) -> Invocation {
    let mut inv = f.invocation("");
    inv.command = command.into();
    inv.flags.remove("--sql");
    if command == "data db2" {
        inv.flags.insert("--table".into(), "RemoteSample".into());
    } else {
        inv.positional = Some("RemoteSample".into());
    }
    inv.flags
        .extend(flags.iter().map(|(k, v)| ((*k).into(), (*v).into())));
    inv
}
#[test]
fn default_remote_sql_and_db2_report_go_memory_boundaries_and_capture_exact_bytes() {
    let f = SqlFixture::new(false, true);
    f.source.backend.calls.lock().unwrap().clear();
    let sql = crate::dispatch(&f.invocation("SELECT ID FROM RemoteSample")).unwrap();
    report(&sql.data);
    assert!(sql.data["sources"][0].get("memoryBudget").is_none());
    f.verify(&sql);
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
        let out = crate::dispatch(&db2(&f, command, &flags)).unwrap();
        if command == "data db2" {
            assert!(out.data.get("memoryBudget").is_none());
        } else {
            report(&out.data);
        }
        f.verify(&out);
        released(&f);
    }
    assert!(f.source.backend.calls.lock().unwrap().is_empty());
    f.close();
}
#[test]
fn actual_remote_sql_reuses_adaptive_budget_after_real_lease_through_execution_and_capture() {
    let f = SqlFixture::new(false, true);
    f.source.backend.calls.lock().unwrap().clear();
    let inv = f.invocation("SELECT ID FROM RemoteSample");
    let calls = Arc::new(AtomicU64::new(0));
    let original_deadline = inv.control.deadline();
    let budget = Arc::new(Budget::query_with_probe(
        original_deadline,
        inv.control.clone(),
        {
            let calls = calls.clone();
            let home = f.source.home.clone();
            Arc::new(move || {
                held(&home);
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(8 << 30)
            })
        },
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let mut executed = false;
    let out = local_query::sql_inner_budgeted(&inv, &budget, &mut || {
        executed = true;
        held(&f.source.home);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        budget.charge_split(Cost::retained(520 << 20), Cost::default())?;
        Ok(())
    })
    .unwrap();
    assert!(executed);
    assert_eq!(out.data["result"]["rows"], json!([[7]]));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(budget.deadline, original_deadline);
    assert_eq!(out.data["memoryBudget"]["growths"], 1);
    assert_eq!(
        out.data["memoryBudget"],
        serde_json::to_value(budget.memory_snapshot().unwrap()).unwrap()
    );
    assert_eq!(
        out.data["resources"],
        serde_json::to_value(budget.logical_snapshot().unwrap()).unwrap()
    );
    assert!(f.source.backend.calls.lock().unwrap().is_empty());
    f.verify(&out);
    released(&f);
    f.close();
}
#[test]
fn actual_remote_sql_independent_budget_cancel_is_not_rewritten_as_invocation_stop() {
    let f = SqlFixture::new(false, true);
    f.source.backend.calls.lock().unwrap().clear();
    let inv = f.invocation("SELECT ID FROM RemoteSample");
    let token = TrackedCancellation::new();
    let control = OperationControl::tracked(inv.control.deadline(), token.clone());
    let budget = Arc::new(Budget::controlled(
        Cost::defaults(),
        control.deadline(),
        control,
    ));
    let before = f.captures().len();
    let mut executed = false;
    let error = local_query::sql_inner_budgeted(&inv, &budget, &mut || {
        executed = true;
        held(&f.source.home);
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
    assert_eq!(f.captures().len(), before);
    released(&f);
    f.close();
}
#[test]
fn default_remote_stream_memory_is_summary_only_and_owner_closes_before_terminal() {
    let f = SqlFixture::new(false, true);
    f.source.backend.calls.lock().unwrap().clear();
    let inv = db2(
        &f,
        "data db2 stream",
        &[("--format", "jsonl"), ("--limit", "1")],
    );
    let mut frames = Vec::new();
    let out = crate::stream::stream(&inv, &mut |frame| {
        if frame["frame"] == "record" {
            held(&f.source.home);
        }
        if frame["frame"] == "end" {
            released(&f);
        }
        frames.push(frame.clone());
        Ok(())
    })
    .unwrap();
    report(&out.data);
    assert_eq!(
        frames
            .iter()
            .map(|v| v["frame"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["begin", "record", "end"]
    );
    for frame in &frames {
        assert!(frame.get("memoryBudget").is_none());
        assert!(frame["begin"].get("memoryBudget").is_none());
        assert!(frame["end"].get("memoryBudget").is_none());
    }
    let raw = evidence::read_blob(&f.source.home, &out.provenance[0]["blob"], 16 << 20).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&raw).unwrap(),
        json!({"begin":frames[0]["begin"],"end":frames[2]["end"]})
    );
    released(&f);
    f.close();
}
#[test]
fn default_remote_asset_inspect_uses_verified_owner_without_new_memory_field() {
    let f = SqlFixture::new(false, true);
    f.source.backend.calls.lock().unwrap().clear();
    let mut inv = f.invocation("");
    inv.command = "asset inspect".into();
    inv.flags.remove("--sql");
    inv.flags.insert("--file-id".into(), "7".into());
    let out = crate::dispatch(&inv).unwrap();
    assert_eq!(out.data["source"], "cdn");
    assert_eq!(out.data["contentVerified"], true);
    assert!(out.data.get("memoryBudget").is_none());
    assert_eq!(out.provenance.len(), 1);
    let raw = evidence::read_blob(&f.source.home, &out.provenance[0]["blob"], 16 << 20).unwrap();
    assert_eq!(raw, f.source.payload);
    assert_eq!(out.provenance[0]["provenance"]["snapshot"], f.snapshot);
    assert!(f.source.backend.calls.lock().unwrap().is_empty());
    released(&f);
    f.close();
}

#[test]
fn actual_remote_asset_capture_id_survives_late_close_with_cli_marker_contract() {
    let f = SqlFixture::new(false, true);
    f.source.backend.calls.lock().unwrap().clear();
    let mut inv = f.invocation("");
    inv.command = "asset inspect".into();
    inv.flags.remove("--sql");
    inv.flags.insert("--file-id".into(), "7".into());
    let primary = fault("records.metadata_limit", 3, "checked Session close failed").with_details(
        json!({"schema":"fixture.close.v1","causes":[{"code":"fixture.reader_close"}]}),
    );
    let mut observed = false;
    let error = local_query::asset_after_finish(&inv, &mut |result| {
        let out = result?;
        assert_eq!(out.provenance.len(), 1);
        observed = true;
        released(&f);
        Err(primary.clone())
    })
    .unwrap_err();
    assert!(observed);
    assert_eq!(f.captures().len(), 1);
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
    let capture = evidence::read_capture(&f.source.home, id).unwrap();
    let raw = evidence::read_blob(&f.source.home, &capture["blob"], 16 << 20).unwrap();
    assert_eq!(raw, f.source.payload);
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
    assert!(f.source.backend.calls.lock().unwrap().is_empty());
    released(&f);
    f.close();
}
