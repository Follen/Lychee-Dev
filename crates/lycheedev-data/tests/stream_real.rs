use lycheedev_core::{Invocation, ToolkitError, control::OperationControl, evidence};
use lycheedev_data::stream;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
fn invocation(home: &std::path::Path, extra: &[(&str, &str)]) -> Invocation {
    let mut flags = BTreeMap::from([
        (
            "--snapshot".into(),
            "PIN-5e5e7a341fdc93b9f965da5682e7b9f69cccd3b7bfc0a179d4d7a2bdf21ac2a3".into(),
        ),
        (
            "--installation".into(),
            "D:/Game/World of Warcraft/_retail_".into(),
        ),
        ("--offline".into(), "true".into()),
        ("--format".into(), "jsonl".into()),
        ("--limit".into(), "200000".into()),
    ]);
    for (name, value) in extra {
        flags.insert(name.to_string(), value.to_string());
    }
    Invocation {
        command: "data db2 stream".into(),
        positional: Some("Map".into()),
        flags,
        home: home.into(),
        cwd: home.into(),
        control: Default::default(),
    }
}
#[test]
#[ignore = "fixed real CASC/DBD inputs and isolated existing proof home required"]
fn real_typed_stream_complete_filtered_projected_error_and_cancel() {
    let home =
        PathBuf::from(std::env::var("LYCHEEDEV_RUST_DATA_CLI_HOME").expect("isolated proof home"));
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
        ("filtered", vec![("--filter", "ID=00")]),
    ] {
        let mut frames = vec![];
        let outcome = stream::stream(&invocation(&home, &extra), &mut |frame| {
            frames.push(frame.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(frames.first().unwrap()["frame"], "begin");
        assert_eq!(frames.last().unwrap()["frame"], "end");
        let end = &frames.last().unwrap()["end"];
        let expected = match name {
            "complete" => 1184,
            "filtered" => 1,
            _ => 3,
        };
        assert_eq!(end["count"], expected);
        assert_eq!(frames.len(), expected as usize + 2);
        assert_eq!(end["complete"], name == "complete" || name == "filtered");
        if name == "projected" {
            assert_eq!(frames[1]["record"]["row"].as_object().unwrap().len(), 2);
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
    let mut inv = invocation(&home, &[]);
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
    let error = stream::stream(&invocation(&home, &[]), &mut |frame| {
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
    let error = stream::stream(&invocation(&home, &[("--filter", "Nope=1")]), &mut |_| {
        started = true;
        Ok(())
    })
    .unwrap_err();
    assert!(!started);
    assert_eq!(error.exit_code, 2);
    reports.push(json!({"case":"bad_field_before_begin","exit":error.exit_code,"started":started}));
    std::fs::write(output.join("report.json"),serde_json::to_vec_pretty(&json!({"scope":"Real fixed Retail SDK provider; not public CLI pipe/control acceptance","cases":reports})).unwrap()).unwrap();
}
