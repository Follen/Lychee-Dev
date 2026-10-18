//! Shared assertions exercise actual local and default offline CDN consumers.
use lycheedev_core::{Invocation, Outcome, evidence};
use lycheedev_data::{db2, local_query, stream};
use serde_json::{Value, json};
fn invocation(base: &Invocation, command: &str, flags: &[(&str, &str)]) -> Invocation {
    let mut inv = base.clone();
    inv.command = command.into();
    inv.positional = (command.starts_with("data db2 ")).then(|| "Sample".into());
    inv.flags
        .extend(flags.iter().map(|(k, v)| ((*k).into(), (*v).into())));
    inv
}
fn captured(base: &Invocation, out: &Outcome) {
    assert!(!out.complete);
    assert!(!out.provenance.is_empty());
    for capture in &out.provenance {
        assert_eq!(capture["complete"], false);
        let raw = evidence::read_blob(&base.home, &capture["blob"], 16 << 20).unwrap();
        if capture["provenance"]["kind"] == "data-query-csv" {
            assert_eq!(raw, b"ID,Value\n1,42\n");
        } else {
            let document: Value = serde_json::from_slice(&raw).unwrap();
            assert!(document["complete"] == false || document["end"]["complete"] == false);
        }
    }
}
pub fn exercise(base: &Invocation) {
    let search = db2::dispatch(&invocation(
        base,
        "data db2 search",
        &[("--field", "Value"), ("--query", ""), ("--limit", "10")],
    ))
    .unwrap();
    assert_eq!(search.data["rows"], json!([{"ID":1,"Value":42}]));
    assert_eq!(search.data["partial"], true);
    assert_eq!(search.data["tables"][0]["contentVerified"], false);
    assert_eq!(
        search.data["tables"][0]["unavailablePartitions"][0]["index"],
        1
    );
    captured(base, &search);
    let schema = db2::dispatch(&invocation(base, "data db2 schema", &[])).unwrap();
    assert_eq!(schema.data["rowCount"], 1);
    assert_eq!(schema.data["partial"], true);
    captured(base, &schema);
    let page = db2::dispatch(&invocation(
        base,
        "data db2",
        &[("--table", "Sample"), ("--limit", "10")],
    ))
    .unwrap();
    assert_eq!(page.data["page"]["rows"], json!([{"ID":1,"Value":42}]));
    let file = &page.data["file"];
    assert_eq!(file["contentVerified"], false);
    assert_eq!(file["verification"]["contentWholeDecodedCKey"], false);
    assert_eq!(file["content"], json!({"sha256":"", "bytes":0}));
    assert!(file["partialContent"]["sha256"].is_string());
    assert_eq!(file["missing"][0]["bytes"], 8);
    assert_eq!(file["missing"][0]["keyID"], "1234567890abcdef");
    assert_eq!(
        page.data["unavailablePartitions"][0]["keyID"],
        "1234567890abcdef"
    );
    captured(base, &page);
    let missing = db2::dispatch(&invocation(
        base,
        "data db2",
        &[("--table", "Sample"), ("--id", "2")],
    ))
    .unwrap_err();
    assert_eq!(missing.code, "container.key_unavailable");
    let count = local_query::sql(&invocation(
        base,
        "data sql",
        &[("--sql", "SELECT COUNT(*) AS n FROM Sample")],
    ))
    .unwrap();
    assert_eq!(count.data["result"]["rows"], json!([[1]]));
    assert_eq!(count.data["partial"], true);
    assert_eq!(count.data["sources"][0]["file"]["contentVerified"], false);
    assert_eq!(
        count.data["sources"][0]["unavailablePartitions"][0]["index"],
        1
    );
    assert!(
        count.data["sources"][0]["file"]
            .get("unavailablePartitions")
            .is_none()
    );
    captured(base, &count);
    let rows = local_query::sql(&invocation(
        base,
        "data sql",
        &[("--sql", "SELECT ID, Value FROM Sample")],
    ))
    .unwrap();
    assert_eq!(rows.data["result"]["rows"], json!([[1, 42]]));
    captured(base, &rows);
    let sql_missing = local_query::sql(&invocation(
        base,
        "data sql",
        &[("--sql", "SELECT ID FROM Sample WHERE ID=2")],
    ))
    .unwrap();
    assert_eq!(sql_missing.data["result"]["rows"], json!([]));
    assert_eq!(sql_missing.data["partial"], true);
    captured(base, &sql_missing);
    let output = base.home.join("partial.csv");
    let csv = local_query::sql(&invocation(
        base,
        "data sql",
        &[
            ("--sql", "SELECT ID, Value FROM Sample"),
            ("--encoding", "csv"),
            ("--output", output.to_str().unwrap()),
        ],
    ))
    .unwrap();
    captured(base, &csv);
    assert_eq!(
        std::fs::read_to_string(&output).unwrap(),
        "ID,Value\n1,42\n"
    );
    let manifest: Value = serde_json::from_slice(
        &std::fs::read(base.home.join("partial.csv.manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["complete"], false);
    let mut frames = Vec::new();
    let streamed = stream::stream(
        &invocation(
            base,
            "data db2 stream",
            &[("--limit", "10"), ("--format", "jsonl")],
        ),
        &mut |frame| {
            frames.push(frame.clone());
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[1]["record"]["row"], json!({"ID":1,"Value":42}));
    assert_eq!(frames[2]["end"]["count"], 1);
    assert_eq!(frames[2]["end"]["complete"], false);
    assert_eq!(frames[2]["end"]["partial"], true);
    assert_eq!(frames[2]["end"]["truncated"], false);
    assert_eq!(
        frames[0]["begin"]["tables"][0]["unavailablePartitions"][0]["index"],
        1
    );
    captured(base, &streamed);
    exercise_stream_stop(base);
}
fn capture_count(base: &Invocation) -> usize {
    let metadata = lycheedev_core::metadata::Metadata::open(&base.home, false).unwrap();
    let count = metadata.list::<Value>("capture/", 100).unwrap().len();
    metadata.close().unwrap();
    count
}
fn exercise_stream_stop(base: &Invocation) {
    let before = capture_count(base);
    for cancel in [true, false] {
        let mut inv = invocation(
            base,
            "data db2 stream",
            &[("--limit", "10"), ("--format", "jsonl")],
        );
        inv.control = Default::default();
        let stop = inv.control.cancel_handle();
        let injected =
            lycheedev_core::ToolkitError::io(std::io::Error::other("partial stream sink stopped"));
        let mut frames = Vec::new();
        let error = stream::stream(&inv, &mut |frame| {
            frames.push(frame.clone());
            if frame["frame"] == "record" {
                if cancel {
                    stop.store(true, std::sync::atomic::Ordering::Release);
                } else {
                    return Err(injected.clone());
                }
            }
            Ok(())
        })
        .unwrap_err();
        assert_eq!(frames[0]["frame"], "begin");
        assert_eq!(frames[1]["record"]["row"], json!({"ID":1,"Value":42}));
        if cancel {
            assert_eq!(error.exit_code, 7);
            assert_eq!(frames.len(), 3);
            assert_eq!(frames[2]["frame"], "error");
            assert_eq!(frames[2]["error"]["code"], "records.cancelled");
        } else {
            assert_eq!(error.code, injected.code);
            assert_eq!(error.message, injected.message);
            assert_eq!(frames.len(), 2, "failed sink was called again");
        }
        assert!(frames.iter().all(|frame| frame["frame"] != "end"));
        assert_eq!(capture_count(base), before);
    }
    let mut inv = invocation(
        base,
        "data db2 stream",
        &[("--limit", "1"), ("--format", "jsonl")],
    );
    inv.control = Default::default();
    let mut frames = Vec::new();
    let fresh = stream::stream(&inv, &mut |frame| {
        frames.push(frame.clone());
        Ok(())
    })
    .unwrap();
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[2]["end"]["partial"], true);
    assert_eq!(
        frames[2]["end"]["truncated"], false,
        "missing rows do not prove truncation"
    );
    captured(base, &fresh);
}
pub fn complete_page(base: &Invocation) {
    let page = db2::dispatch(&invocation(
        base,
        "data db2",
        &[("--table", "Sample"), ("--limit", "10")],
    ))
    .unwrap();
    assert!(page.complete);
    assert_eq!(
        page.data["page"]["rows"],
        json!([{"ID":1,"Value":42},{"ID":2,"Value":99}])
    );
    assert_eq!(page.data["file"]["contentVerified"], true);
    assert!(page.data["file"].get("partialContent").is_none());
}
