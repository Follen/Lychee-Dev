//! Real default JSONL consumer, with the same authenticated offline SQL fixture.
use super::*;
use crate::stream;
use lycheedev_core::ToolkitError;

fn invocation(f: &SqlFixture, flags: &[(&str, &str)]) -> Invocation {
    let mut inv = f.invocation("");
    inv.command = "data db2 stream".into();
    inv.positional = Some("RemoteSample".into());
    inv.flags.remove("--sql");
    inv.flags.insert("--format".into(), "jsonl".into());
    inv.flags.insert("--limit".into(), "1".into());
    inv.flags
        .extend(flags.iter().map(|(k, v)| ((*k).into(), (*v).into())));
    inv
}
fn frames(values: &[Value]) -> Vec<&str> {
    values
        .iter()
        .map(|value| value["frame"].as_str().unwrap())
        .collect()
}
fn writer_error() -> ToolkitError {
    fault("external.io", 5, "fixture sink failed")
}
fn verify_manifest(f: &SqlFixture, capture: &Value, begin: &Value, end: &Value) {
    assert_eq!(capture["provenance"]["snapshot"], f.snapshot);
    assert_eq!(capture["provenance"]["kind"], "data-stream-manifest");
    let raw = evidence::read_blob(&f.source.home, &capture["blob"], 16 << 20).unwrap();
    assert_eq!(storage::sha256(&raw), capture["blob"]["sha256"]);
    assert_eq!(
        serde_json::from_slice::<Value>(&raw).unwrap(),
        json!({"begin":begin,"end":end})
    );
}

#[test]
fn default_remote_stream_yields_exact_typed_frames_and_captures_only_manifest() {
    let f = SqlFixture::new(false, true);
    for (flags, count) in [
        (vec![], 1),
        (vec![("--filter", "ID=8")], 0),
        (vec![("--fields", "ID")], 1),
    ] {
        let mut yielded = Vec::new();
        let out = stream::stream(&invocation(&f, &flags), &mut |frame| {
            yielded.push(frame.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(yielded.len(), count + 2);
        assert_eq!(yielded[0]["frame"], "begin");
        assert_eq!(yielded[0]["begin"]["source"], "cdn");
        assert_eq!(yielded[0]["begin"]["tables"][0]["contentVerified"], true);
        assert_eq!(yielded.last().unwrap()["frame"], "end");
        assert_eq!(yielded.last().unwrap()["end"]["count"], count);
        assert_eq!(yielded.last().unwrap()["end"]["complete"], true);
        assert_eq!(out.data["source"], "cdn");
        assert_eq!(out.data["resourceAccounting"]["used"]["networkRequests"], 0);
        if count == 1 {
            assert_eq!(yielded[1]["record"]["recordID"], 7);
            assert_eq!(yielded[1]["record"]["row"]["ID"], 7);
            if flags.iter().any(|(key, _)| *key == "--fields") {
                assert_eq!(yielded[1]["record"]["row"], json!({"ID":7}));
                assert!(yielded[1]["record"]["row"].get("Name").is_none());
            }
        }
        verify_manifest(
            &f,
            &out.provenance[0],
            &yielded[0]["begin"],
            &yielded.last().unwrap()["end"],
        );
    }
    f.close();
}

#[test]
fn default_remote_stream_never_calls_a_failed_begin_record_or_end_sink_again() {
    for fail_at in 1..=3 {
        let f = SqlFixture::new(false, true);
        let inv = invocation(&f, &[]);
        let mut attempted = Vec::new();
        let error = stream::stream(&inv, &mut |frame| {
            attempted.push(frame.clone());
            if attempted.len() == fail_at {
                Err(writer_error())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert_eq!(error.code, "external.io");
        assert_eq!(attempted.len(), fail_at);
        assert!(!frames(&attempted).contains(&"error"));
        assert_eq!(f.captures().len(), usize::from(fail_at == 3));
        if fail_at == 3 {
            let capture = &error.details["completedCaptures"][0];
            assert!(error.message.contains(capture["id"].as_str().unwrap()));
            verify_manifest(&f, capture, &attempted[0]["begin"], &attempted[2]["end"]);
        }
        f.close();
    }
}

#[test]
fn default_remote_stream_cancellation_after_yield_has_one_error_no_capture_or_end() {
    let f = SqlFixture::new(false, true);
    let inv = invocation(&f, &[]);
    let mut yielded = Vec::new();
    let error = stream::stream(&inv, &mut |frame| {
        yielded.push(frame.clone());
        if frame["frame"] == "record" {
            inv.control.cancel_handle().store(true, Ordering::Release);
        }
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.exit_code, 7);
    assert_eq!(frames(&yielded), ["begin", "record", "error"]);
    assert_eq!(yielded[2]["error"]["code"], "records.cancelled");
    assert!(f.captures().is_empty());
    lycheedev_core::workspace::data_resources::acquire(
        &f.source.home,
        Policy::Measurement,
        &OperationControl::default(),
    )
    .unwrap()
    .close()
    .unwrap();
    f.close();
}

#[test]
fn default_remote_stream_preserves_business_and_failed_error_marker_causes() {
    let f = SqlFixture::new(false, true);
    let inv = invocation(&f, &[]);
    let mut attempted = Vec::new();
    let error = stream::stream(&inv, &mut |frame| {
        attempted.push(frame.clone());
        if frame["frame"] == "record" {
            inv.control.cancel_handle().store(true, Ordering::Release);
        }
        if frame["frame"] == "error" {
            Err(writer_error())
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error.exit_code, 7);
    assert_eq!(frames(&attempted), ["begin", "record", "error"]);
    assert_eq!(error.details["schema"], "lycheedev.stream-errors.v1");
    assert_eq!(error.details["causes"].as_array().unwrap().len(), 2);
    assert_eq!(error.details["causes"][0]["exit_code"], 7);
    assert_eq!(error.details["causes"][1]["code"], "external.io");
    assert!(f.captures().is_empty());
    f.close();
}

#[test]
fn default_remote_stream_prebegin_failure_is_silent_and_row_failure_has_no_end() {
    let f = SqlFixture::new(false, true);
    let mut yielded = Vec::new();
    let error = stream::stream(&invocation(&f, &[("--fields", "Missing")]), &mut |frame| {
        yielded.push(frame.clone());
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.code, "records.field_unknown");
    assert!(yielded.is_empty());
    assert!(f.captures().is_empty());
    f.close();
    let f = SqlFixture::new(true, true);
    let error = stream::stream(&invocation(&f, &[]), &mut |frame| {
        yielded.push(frame.clone());
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.code, "table.invalid_format");
    assert_eq!(frames(&yielded), ["begin", "error"]);
    assert!(f.captures().is_empty());
    f.close();
}

#[test]
fn default_remote_stream_closes_admission_after_capture_before_success_terminal() {
    let f = SqlFixture::new(false, true);
    let mut observed = Vec::new();
    let out = stream::stream(&invocation(&f, &[]), &mut |frame| {
        let phase = frame["frame"].as_str().unwrap();
        let bound = OperationControl::uncancelable(Instant::now() + Duration::from_millis(50));
        let measurement = lycheedev_core::workspace::data_resources::acquire(
            &f.source.home,
            Policy::Measurement,
            &bound,
        );
        if phase == "end" {
            measurement.unwrap().close().unwrap();
            assert_eq!(f.captures().len(), 1);
        } else {
            assert!(measurement.is_err());
        }
        observed.push(frame.clone());
        Ok(())
    })
    .unwrap();
    assert_eq!(frames(&observed), ["begin", "record", "end"]);
    assert_eq!(out.provenance.len(), 1);
    f.close();
}

#[test]
fn default_remote_stream_preserves_published_capture_on_injected_checked_close_failure() {
    let f = SqlFixture::new(false, true);
    let inv = invocation(&f, &[]);
    let mut yielded = Vec::new();
    let mut finishes = 0;
    let error=stream::stream_after_finish(&inv,&mut |frame| {yielded.push(frame.clone());Ok(())},&mut |result| {
        assert!(result.is_ok());
        finishes+=1;
        // Production owners have really consumed/closed. The fault is injected
        // only into that checked result, not into table IO or publication.
        lycheedev_core::workspace::data_resources::acquire(&f.source.home,Policy::Measurement,&OperationControl::default()).unwrap().close().unwrap();
        let mut close=fault("vault.close_failed",5,"fixture checked close failed");
        close.details=json!({"schema":"fixture.close-causes","causes":[
            fault("vault.close_failed",5,"first close"),fault("vault.close_failed",5,"second close")
        ]});
        Err(close)
    }).unwrap_err();
    assert_eq!(finishes, 1);
    assert_eq!(frames(&yielded), ["begin", "record", "error"]);
    assert_eq!(f.captures().len(), 1);
    assert_eq!(error.code, "vault.close_failed");
    assert_eq!(
        error.details["primary"]["details"]["causes"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let capture = &error.details["completedCaptures"][0];
    assert_eq!(
        evidence::read_capture(&f.source.home, capture["id"].as_str().unwrap()).unwrap(),
        *capture
    );
    assert!(
        yielded[2]["error"]["message"]
            .as_str()
            .unwrap()
            .contains(capture["id"].as_str().unwrap())
    );
    assert_eq!(
        yielded[2]["error"].as_object().unwrap().len(),
        2,
        "fixed public code/message only"
    );
    f.close();
}

#[test]
fn default_remote_stream_limit_probes_a_second_real_row_without_yielding_it() {
    let f = SqlFixture::with_rows(false, true, 3);
    let mut yielded = Vec::new();
    let out = stream::stream(&invocation(&f, &[]), &mut |frame| {
        yielded.push(frame.clone());
        Ok(())
    })
    .unwrap();
    assert_eq!(frames(&yielded), ["begin", "record", "end"]);
    assert_eq!(yielded[1]["record"]["recordID"], 7);
    assert_eq!(
        yielded[2]["end"],
        json!({"partial":false,"partialReasons":null,"count":1,"complete":false,"truncated":true,"truncationReason":"limit_reached"})
    );
    assert!(!out.complete);
    assert_eq!(out.data["count"], 1);
    verify_manifest(
        &f,
        &out.provenance[0],
        &yielded[0]["begin"],
        &yielded[2]["end"],
    );
    yielded.clear();
    let out = stream::stream(&invocation(&f, &[("--filter", "ID=8")]), &mut |frame| {
        yielded.push(frame.clone());
        Ok(())
    })
    .unwrap();
    assert_eq!(frames(&yielded), ["begin", "record", "end"]);
    assert_eq!(yielded[1]["record"]["recordID"], 8);
    assert_eq!(yielded[2]["end"]["complete"], true);
    assert!(out.complete);
    f.close();
}

#[test]
fn default_remote_stream_keeps_capture_and_multiple_close_causes_when_error_marker_also_fails() {
    let f = SqlFixture::new(false, true);
    let inv = invocation(&f, &[]);
    let mut attempted = Vec::new();
    let mut finishes = 0;
    let error=stream::stream_after_finish(&inv,&mut |frame| {
        attempted.push(frame.clone());
        if frame["frame"]=="error" {Err(writer_error())} else {Ok(())}
    },&mut |result| {
        assert!(result.is_ok());
        finishes+=1;
        let mut close=fault("vault.close_failed",5,"fixture checked close failed");
        close.details=json!({"schema":"fixture.close-causes","causes":[
            fault("vault.close_failed",5,"first close"),fault("vault.close_failed",5,"second close")
        ]});
        Err(close)
    }).unwrap_err();
    assert_eq!(finishes, 1);
    assert_eq!(frames(&attempted), ["begin", "record", "error"]);
    assert_eq!(f.captures().len(), 1);
    assert_eq!(error.code, "vault.close_failed");
    assert_eq!(error.details["schema"], "lycheedev.stream-errors.v1");
    let causes = error.details["causes"].as_array().unwrap();
    assert_eq!(causes.len(), 2);
    assert_eq!(
        causes[0]["details"]["primary"]["details"]["causes"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(causes[1]["code"], "external.io");
    let capture = &causes[0]["details"]["completedCaptures"][0];
    assert_eq!(
        evidence::read_capture(&f.source.home, capture["id"].as_str().unwrap()).unwrap(),
        *capture
    );
    assert!(
        attempted[2]["error"]["message"]
            .as_str()
            .unwrap()
            .contains(capture["id"].as_str().unwrap())
    );
    assert_eq!(attempted[2]["error"].as_object().unwrap().len(), 2);
    f.close();
}

#[test]
fn default_remote_stream_native_service_exposes_typed_rows_and_checked_summary() {
    use stream::{StreamEvent, StreamRequest};
    let f = SqlFixture::with_rows(false, true, 3);
    let inv = invocation(&f, &[("--fields", "ID,ID"), ("--filter", " ID = 08 ")]);
    let mut yielded = Vec::new();
    let mut records = 0;
    let summary = stream::run(
        StreamRequest::from_invocation(&inv).unwrap(),
        &mut |event| {
            match event {
                StreamEvent::Begin(begin) => {
                    assert_eq!(begin.snapshot, f.snapshot);
                    assert_eq!(begin.filter, " ID = 08 ");
                    assert_eq!(begin.projection.fields(), ["ID", "ID"]);
                    assert_eq!(begin.limit.get(), 1);
                }
                StreamEvent::Record(record) => {
                    assert_eq!(record.index, 0);
                    assert_eq!(record.record_id, 8);
                    assert_eq!(record.row.len(), 1);
                    assert_eq!(record.row["ID"], crate::table::Value::UInt(8));
                    records += 1;
                }
                StreamEvent::End(walk) => {
                    assert_eq!(walk.count, 1);
                    assert!(walk.complete());
                    assert_eq!(f.captures().len(), 1);
                    // The same admission must really be gone before native End.
                    lycheedev_core::workspace::data_resources::acquire(
                        &f.source.home,
                        Policy::Measurement,
                        &OperationControl::default(),
                    )
                    .unwrap()
                    .close()
                    .unwrap();
                }
                StreamEvent::Error { .. } => panic!("successful fixture emitted error"),
            }
            yielded.push(stream::wire::event(event));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(records, 1);
    assert_eq!(summary.walk.count, records);
    assert_eq!(summary.begin.projection.fields(), ["ID", "ID"]);
    assert_eq!(summary.captures.len(), 1);
    assert_eq!(summary.resource_usage.network_requests, 0);
    assert_eq!(frames(&yielded), ["begin", "record", "end"]);
    verify_manifest(
        &f,
        &summary.captures[0],
        &yielded[0]["begin"],
        &yielded[2]["end"],
    );
    let out = stream::wire::into_outcome(summary);
    assert_eq!(out.data["filter"], " ID = 08 ");
    assert_eq!(out.data["fields"], json!(["ID", "ID"]));
    assert!(out.complete);
    f.close();
}

#[test]
fn default_remote_stream_native_service_never_reinvokes_failed_sink_and_keeps_capture() {
    use stream::{StreamEvent, StreamRequest};
    for fail_at in 1..=3 {
        let f = SqlFixture::new(false, true);
        let inv = invocation(&f, &[]);
        let mut calls = 0;
        let error = stream::run(
            StreamRequest::from_invocation(&inv).unwrap(),
            &mut |event| {
                assert!(!matches!(event, StreamEvent::Error { .. }));
                calls += 1;
                if calls == fail_at {
                    Err(writer_error())
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
        assert_eq!(calls, fail_at);
        assert_eq!(error.code, "external.io");
        assert_eq!(f.captures().len(), usize::from(fail_at == 3));
        if fail_at == 3 {
            assert_eq!(error.details["primary"]["code"], "external.io");
            let capture = &error.details["completedCaptures"][0];
            assert_eq!(
                evidence::read_capture(&f.source.home, capture["id"].as_str().unwrap()).unwrap(),
                *capture
            );
        }
        f.close();
    }
}

#[test]
fn default_remote_stream_native_cancellation_preserves_one_terminal_error() {
    use stream::{StreamEvent, StreamRequest};
    let f = SqlFixture::new(false, true);
    let inv = invocation(&f, &[]);
    let mut phases = Vec::new();
    let error = stream::run(
        StreamRequest::from_invocation(&inv).unwrap(),
        &mut |event| {
            phases.push(match event {
                StreamEvent::Begin(_) => "begin",
                StreamEvent::Record(_) => {
                    inv.control.cancel_handle().store(true, Ordering::Release);
                    "record"
                }
                StreamEvent::End(_) => panic!("cancelled stream cannot end successfully"),
                StreamEvent::Error { code, .. } => {
                    assert_eq!(code, "records.cancelled");
                    "error"
                }
            });
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error.exit_code, 7);
    assert_eq!(phases, ["begin", "record", "error"]);
    assert!(f.captures().is_empty());
    f.close();
}

#[test]
fn default_remote_stream_provider_error_precedes_deferred_projection_validation() {
    let f = SqlFixture::new(false, false);
    let inv = invocation(&f, &[("--fields", "Missing")]);
    let request = stream::StreamRequest::from_invocation(&inv).unwrap();
    let mut calls = 0;
    let error = stream::run(request, &mut |_| {
        calls += 1;
        Ok(())
    })
    .unwrap_err();
    assert_eq!(calls, 0);
    assert_eq!(error.code, "records.remote_unavailable_offline");
    assert!(f.captures().is_empty());
    f.close();
}

#[test]
fn default_remote_stream_checks_filter_before_projection_without_yield_or_capture() {
    let f = SqlFixture::new(false, true);
    for (fields, filter, expected) in [
        ("Missing", "Other=1", "Other"),
        ("Missing", "ID=7", "Missing"),
        (" ID", "ID=7", " ID"),
        ("ID,", "ID=7", ""),
    ] {
        let inv = invocation(&f, &[("--fields", fields), ("--filter", filter)]);
        let mut yielded = Vec::new();
        let error = stream::stream(&inv, &mut |frame| {
            yielded.push(frame.clone());
            Ok(())
        })
        .unwrap_err();
        assert_eq!(error.code, "records.field_unknown");
        assert_eq!(
            error.message,
            if expected.is_empty() {
                "records.field_unknown".to_owned()
            } else {
                format!("records.field_unknown: {expected}")
            }
        );
        assert!(yielded.is_empty());
        assert!(f.captures().is_empty());
    }
    f.close();
}

#[test]
fn default_remote_stream_unsigned_filter_rejects_sign_like_go_parse_uint() {
    let f = SqlFixture::with_rows(false, true, 3);
    let inv = invocation(&f, &[("--filter", "ID=+08")]);
    let mut yielded = Vec::new();
    let out = stream::stream(&inv, &mut |frame| {
        yielded.push(frame.clone());
        Ok(())
    })
    .unwrap();
    assert_eq!(frames(&yielded), ["begin", "end"]);
    assert_eq!(yielded[0]["begin"]["filter"], "ID=+08");
    assert_eq!(yielded[1]["end"]["count"], 0);
    assert_eq!(yielded[1]["end"]["complete"], true);
    assert_eq!(f.captures().len(), 1);
    verify_manifest(
        &f,
        &out.provenance[0],
        &yielded[0]["begin"],
        &yielded[1]["end"],
    );
    f.close();
}
