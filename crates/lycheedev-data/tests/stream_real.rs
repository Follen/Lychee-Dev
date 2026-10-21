#[path = "support/current_local_data.rs"]
mod current_local_data;
use lycheedev_core::{Invocation, ToolkitError, control::OperationControl, evidence};
use lycheedev_data::stream;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
fn invocation(
    fixture: &current_local_data::Fixture,
    home: &std::path::Path,
    pin: &str,
    extra: &[(&str, &str)],
) -> Invocation {
    let mut inv = fixture.invocation(
        home,
        pin,
        "data db2 stream",
        Some("Map"),
        &[("--format", "jsonl"), ("--limit", "200000")],
    );
    for (name, value) in extra {
        inv.flags.insert((*name).into(), (*value).into());
    }
    inv
}
#[test]
#[ignore = "current Go-authenticated CASC/DBD fixture and fresh proof home required"]
fn real_typed_stream_complete_filtered_projected_error_and_cancel() {
    let fixture = current_local_data::Fixture::load();
    let home = PathBuf::from(
        std::env::var("LYCHEEDEV_RUST_DATA_CLI_HOME").expect("fresh isolated proof home"),
    );
    let pin = fixture.initialize(&home);
    let oracle = fixture.rows();
    let rows = oracle["rows"].as_array().unwrap();
    let record_id = fixture.record_id();
    let filter = format!("ID=0{record_id}");
    let output = home.parent().unwrap().join("stream-sdk");
    std::fs::create_dir_all(&output).unwrap();
    let mut reports = vec![];
    for (name, extra) in [
        ("complete", vec![]),
        ("truncated", vec![("--limit", "3")]),
        (
            "projected",
            vec![("--limit", "3"), ("--fields", "ID,Directory")],
        ),
        ("filtered", vec![("--filter", filter.as_str())]),
    ] {
        let mut frames = vec![];
        let outcome = stream::stream(&invocation(&fixture, &home, &pin, &extra), &mut |frame| {
            frames.push(frame.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(frames.first().unwrap()["frame"], "begin");
        assert_eq!(frames.last().unwrap()["frame"], "end");
        let end = &frames.last().unwrap()["end"];
        let selected = match name {
            "complete" => rows.iter().collect::<Vec<_>>(),
            "filtered" => rows
                .iter()
                .filter(|row| row["ID"].as_u64().unwrap().to_string() == record_id)
                .collect(),
            _ => rows.iter().take(3).collect(),
        };
        assert!(!selected.is_empty());
        assert_eq!(end["count"], selected.len());
        assert_eq!(frames.len(), selected.len() + 2);
        assert_eq!(end["complete"], name == "complete" || name == "filtered");
        assert_eq!(end["truncated"], name == "truncated" || name == "projected");
        for (index, expected_row) in selected.iter().enumerate() {
            let record = &frames[index + 1];
            assert_eq!(record["frame"], "record");
            assert_eq!(record["record"]["index"], index);
            assert_eq!(record["record"]["recordID"], expected_row["ID"]);
            let expected_row = if name == "projected" {
                json!({"ID":expected_row["ID"],"Directory":expected_row["Directory"]})
            } else {
                (*expected_row).clone()
            };
            assert_eq!(record["record"]["row"], expected_row, "{name} row {index}");
        }
        for cap in &outcome.provenance {
            let doc = evidence::read_capture(&home, cap["id"].as_str().unwrap()).unwrap();
            evidence::read_blob(&home, &doc["blob"], 16 << 20).unwrap();
        }
        let raw = frames
            .iter()
            .map(|f| serde_json::to_string(f).unwrap() + "\n")
            .collect::<String>();
        std::fs::write(output.join(format!("rust-{name}.jsonl")), raw).unwrap();
        reports.push(json!({"case":name,"frames":frames.len(),"end":end,"outcome":outcome}));
    }
    let mut inv = invocation(&fixture, &home, &pin, &[]);
    let cancel = Arc::new(AtomicBool::new(false));
    inv.control = OperationControl::new(Instant::now() + Duration::from_secs(180), cancel.clone());
    let mut frames = vec![];
    let error = stream::stream(&inv, &mut |frame| {
        frames.push(frame.clone());
        if frame["frame"] == "record" {
            cancel.store(true, Ordering::Release)
        }
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.exit_code, 7);
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[2]["frame"], "error");
    assert_eq!(frames[2]["error"]["code"], "records.cancelled");
    assert!(!frames.iter().any(|f| f["frame"] == "end"));
    reports.push(json!({"case":"caller_cancel","frames":frames,"errorCode":error.code,"exit":error.exit_code}));
    let mut accepted: Vec<Value> = vec![];
    let mut attempts = 0;
    let error = stream::stream(&invocation(&fixture, &home, &pin, &[]), &mut |frame| {
        attempts += 1;
        if attempts > 1 {
            return Err(ToolkitError::new("external.io", "test sink BrokenPipe", 5));
        }
        accepted.push(frame.clone());
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.exit_code, 5);
    assert_eq!(accepted.len(), 1);
    // Fixed Go stops calling a sink immediately after its first failure.
    assert_eq!(attempts, 2);
    reports.push(json!({"case":"broken_sink","accepted":accepted,"attempts":attempts,"exit":error.exit_code}));
    let mut started = false;
    let error = stream::stream(
        &invocation(&fixture, &home, &pin, &[("--filter", "Nope=1")]),
        &mut |_| {
            started = true;
            Ok(())
        },
    )
    .unwrap_err();
    assert!(!started);
    assert_eq!(error.exit_code, 2);
    reports.push(json!({"case":"bad_field_before_begin","exit":error.exit_code,"started":started}));
    std::fs::write(output.join("report.json"),serde_json::to_vec_pretty(&json!({"scope":"Current installed client with independently frozen Go324 rows; SDK provider, not public CLI pipe/control acceptance","cases":reports})).unwrap()).unwrap();
}
