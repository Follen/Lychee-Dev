//! Shared actual local/CDN assertions for the cross-section string-pool fixture.
use lycheedev_core::{Invocation, Outcome, evidence, metadata::Metadata};
use lycheedev_data::{db2, local_query, stream};
use serde_json::{Value, json};
fn invocation(base: &Invocation, command: &str, flags: &[(&str, &str)]) -> Invocation {
    let mut inv = base.clone();
    inv.command = command.into();
    inv.positional = command.starts_with("data db2 ").then(|| "Sample".into());
    inv.flags
        .extend(flags.iter().map(|(k, v)| ((*k).into(), (*v).into())));
    inv
}
fn captured(base: &Invocation, out: &Outcome) {
    assert!(!out.complete);
    assert!(!out.provenance.is_empty());
    for capture in &out.provenance {
        assert_eq!(capture["complete"], false);
        assert_eq!(
            evidence::read_capture(&base.home, capture["id"].as_str().unwrap()).unwrap(),
            *capture
        );
        let raw = evidence::read_blob(&base.home, &capture["blob"], 16 << 20).unwrap();
        if capture["provenance"]["kind"] == "data-query-csv" {
            assert_eq!(raw, b"ID,Name\n1,visible\n");
        } else {
            let body: Value = serde_json::from_slice(&raw).unwrap();
            assert!(body["complete"] == false || body["end"]["complete"] == false);
        }
    }
}
pub fn exercise(base: &Invocation, gap_offset: usize, readable: bool) {
    let mut attempts = vec![
        invocation(base, "data db2", &[("--table", "Sample"), ("--id", "1")]),
        invocation(
            base,
            "data db2",
            &[("--table", "Sample"), ("--limit", "10")],
        ),
        invocation(
            base,
            "data db2 search",
            &[
                ("--field", "Name"),
                ("--query", "visible"),
                ("--limit", "10"),
            ],
        ),
        invocation(
            base,
            "data sql",
            &[("--sql", "SELECT ID, Name FROM Sample")],
        ),
    ];
    if !readable {
        for inv in attempts {
            let error = if inv.command == "data sql" {
                local_query::sql(&inv)
            } else {
                db2::dispatch(&inv)
            }
            .unwrap_err();
            assert_eq!(error.code, "table.partition_unavailable");
        }
        let mut frames = Vec::new();
        let error = stream::stream(
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
        .unwrap_err();
        assert_eq!(error.code, "table.partition_unavailable");
        assert!(
            frames
                .iter()
                .all(|frame| frame["frame"] != "record" && frame["frame"] != "end")
        );
        let metadata = Metadata::open(&base.home, false).unwrap();
        assert!(metadata.list::<Value>("capture/", 100).unwrap().is_empty());
        metadata.close().unwrap();
        return;
    }
    let record = db2::dispatch(&attempts.remove(0)).unwrap();
    assert_eq!(record.data["row"], json!({"ID":1,"Name":"visible"}));
    assert_eq!(record.data["file"]["missing"][0]["offset"], gap_offset);
    assert_eq!(record.data["file"]["missing"][0]["bytes"], 8);
    assert_eq!(record.data["file"]["contentVerified"], false);
    assert_eq!(record.data["unavailablePartitions"][0]["index"], 1);
    captured(base, &record);
    let page = db2::dispatch(&attempts.remove(0)).unwrap();
    assert_eq!(
        page.data["page"]["rows"],
        json!([{"ID":1,"Name":"visible"}])
    );
    captured(base, &page);
    let search = db2::dispatch(&attempts.remove(0)).unwrap();
    assert_eq!(search.data["rows"], json!([{"ID":1,"Name":"visible"}]));
    assert_eq!(search.data["partial"], true);
    captured(base, &search);
    let sql = local_query::sql(&attempts.remove(0)).unwrap();
    assert_eq!(sql.data["result"]["rows"], json!([[1, "visible"]]));
    assert_eq!(sql.data["partial"], true);
    captured(base, &sql);
    let schema = db2::dispatch(&invocation(base, "data db2 schema", &[])).unwrap();
    assert_eq!(schema.data["rowCount"], 1);
    captured(base, &schema);
    let count = local_query::sql(&invocation(
        base,
        "data sql",
        &[("--sql", "SELECT COUNT(*) AS n FROM Sample")],
    ))
    .unwrap();
    assert_eq!(count.data["result"]["rows"], json!([[1]]));
    captured(base, &count);
    let output = base.home.join("partial-string.csv");
    let csv = local_query::sql(&invocation(
        base,
        "data sql",
        &[
            ("--sql", "SELECT ID, Name FROM Sample"),
            ("--encoding", "csv"),
            ("--output", output.to_str().unwrap()),
        ],
    ))
    .unwrap();
    assert_eq!(std::fs::read(&output).unwrap(), b"ID,Name\n1,visible\n");
    captured(base, &csv);
    let mut frames = Vec::new();
    let stream = stream::stream(
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
    assert_eq!(frames[1]["record"]["row"], json!({"ID":1,"Name":"visible"}));
    assert_eq!(frames[2]["end"]["partial"], true);
    assert_eq!(frames[2]["end"]["complete"], false);
    assert_eq!(frames[2]["end"]["truncated"], false);
    captured(base, &stream);
}

pub fn complete(base: &Invocation) {
    let page = db2::dispatch(&invocation(
        base,
        "data db2",
        &[("--table", "Sample"), ("--limit", "10")],
    ))
    .unwrap();
    assert!(page.complete);
    assert_eq!(
        page.data["page"]["rows"],
        json!([{"ID":1,"Name":"visible"},{"ID":2,"Name":""}])
    );
    assert_eq!(page.data["file"]["contentVerified"], true);
    assert_eq!(
        page.data["file"]["verification"]["contentWholeDecodedCKey"],
        true
    );
}
