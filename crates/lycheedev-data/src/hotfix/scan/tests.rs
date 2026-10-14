use super::*;
use lycheedev_core::{
    metadata::Metadata,
    selection::{self, DataPin, SelectionSpec},
    workspace,
};
use std::{collections::BTreeMap, fs, path::PathBuf};

struct Fixture {
    root: PathBuf,
    inv: Invocation,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lychee-hotfix-scan-fault-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let home = root.join("home");
        workspace::initialize(&home).unwrap();
        let pin = selection::pin_selection(
            &home,
            SelectionSpec {
                data: Some(DataPin {
                    product: "retail".into(),
                    region: "cn".into(),
                    language: "zhCN".into(),
                    full_build: "12.1.0.69875".into(),
                    build_config: "a".repeat(32),
                    cdn_config: "b".repeat(32),
                    definition_commit: "c".repeat(40),
                }),
                ..Default::default()
            },
        )
        .unwrap()
        .id;
        let mut raw = b"XFTH".to_vec();
        raw.extend(9u32.to_le_bytes());
        raw.extend(69875u32.to_le_bytes());
        raw.resize(44, 0);
        for id in 1u32..=3 {
            let mut header = [0; 32];
            header[..4].copy_from_slice(b"XFTH");
            header[16..20].copy_from_slice(&42u32.to_le_bytes());
            header[20..24].copy_from_slice(&id.to_le_bytes());
            header[28] = 2;
            raw.extend(header);
        }
        let file = root.join("DBCache.bin");
        fs::write(&file, raw).unwrap();
        let inv = Invocation {
            command: "data hotfix".into(),
            positional: None,
            home,
            cwd: root.clone(),
            control: Default::default(),
            flags: BTreeMap::from([
                ("--source".into(), "dbcache".into()),
                ("--scan".into(), "true".into()),
                ("--snapshot".into(), pin),
                ("--dbcache".into(), file.to_string_lossy().into_owned()),
                ("--limit".into(), "1".into()),
                ("--max-pages".into(), "3".into()),
            ]),
        };
        Self { root, inv }
    }
    fn resume(&self, error: &ToolkitError) -> Invocation {
        let reading = &error.details["result"];
        let mut inv = self.inv.clone();
        inv.control = Default::default();
        inv.flags.remove("--dbcache");
        inv.flags.insert(
            "--from".into(),
            reading["result"]["source"]["id"].as_str().unwrap().into(),
        );
        inv.flags.insert(
            "--snapshot".into(),
            reading["result"]["snapshot"].as_str().unwrap().into(),
        );
        inv.flags.insert(
            "--cursor".into(),
            reading["resume"].as_str().unwrap().into(),
        );
        inv
    }
    fn count_scans(&self) -> usize {
        Metadata::open(&self.inv.home, false)
            .unwrap()
            .list::<Value>("capture/", 1000)
            .unwrap()
            .iter()
            .filter(|value| value["provenance"]["kind"] == "hotfix-scan")
            .count()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn cancellation_after_a_saved_page_returns_that_checkpoint_and_can_resume() {
    let fixture = Fixture::new();
    let cancellation = fixture.inv.control.tracked_cancel_handle().unwrap();
    let mut calls = 0;
    let error = dispatch_with(&fixture.inv, |inv| {
        calls += 1;
        if calls == 2 {
            cancellation.cancel();
        }
        local::dispatch(inv)
    })
    .unwrap_err();
    assert_eq!(error.exit_code, 7);
    assert_eq!(error.details["result"]["result"]["returned"], 1);
    assert_eq!(
        error.details["result"]["result"]["pages"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        error.details["result"]["resume"],
        error.details["result"]["capture"]["id"]
    );
    assert_eq!(fixture.count_scans(), 1);
    let done = dispatch(&fixture.resume(&error)).unwrap();
    assert!(done.complete);
    assert_eq!(done.data["result"]["returned"], 3);
}

#[test]
fn removed_page_before_checkpoint_commit_returns_previous_published_coverage() {
    let fixture = Fixture::new();
    let mut calls = 0;
    let error = dispatch_with(&fixture.inv, |inv| {
        let page = local::dispatch(inv)?;
        calls += 1;
        if calls == 2 {
            evidence::remove_capture(
                &inv.home,
                page.provenance.last().unwrap()["id"].as_str().unwrap(),
            )?;
        }
        Ok(page)
    })
    .unwrap_err();
    assert_eq!(error.code, "vault.record_missing");
    assert_eq!(error.details["result"]["result"]["returned"], 1);
    assert_eq!(fixture.count_scans(), 1);
    let done = dispatch(&fixture.resume(&error)).unwrap();
    assert!(done.complete);
    assert_eq!(done.data["result"]["returned"], 3);
}

#[test]
fn removed_first_source_before_publication_does_not_invent_a_resume() {
    let fixture = Fixture::new();
    let error = dispatch_with(&fixture.inv, |inv| {
        let page = local::dispatch(inv)?;
        evidence::remove_capture(
            &inv.home,
            page.provenance.last().unwrap()["id"].as_str().unwrap(),
        )?;
        evidence::remove_capture(&inv.home, page.data["source"]["id"].as_str().unwrap())?;
        Ok(page)
    })
    .unwrap_err();
    assert_eq!(error.code, "vault.record_missing");
    assert!(error.details.get("result").is_none());
    assert_eq!(fixture.count_scans(), 0);
}

#[test]
fn source_coverage_and_cursor_faults_never_publish_mutated_progress() {
    for fault_kind in [
        "source",
        "matched",
        "backward",
        "next_missing",
        "next_wrong",
        "short_final",
    ] {
        let fixture = Fixture::new();
        let mut calls = 0;
        let error = dispatch_with(&fixture.inv, |inv| {
            let mut page = local::dispatch(inv)?;
            calls += 1;
            if calls == 2 {
                match fault_kind {
                    "source" => page.data["source"]["id"] = json!("CAP-changed"),
                    "matched" => page.data["page"]["matched"] = json!(4),
                    "backward" => page.data["page"]["entries"][0]["index"] = json!(0),
                    "next_missing" => {
                        page.data["page"]
                            .as_object_mut()
                            .unwrap()
                            .remove("nextIndex");
                    }
                    "next_wrong" => page.data["page"]["nextIndex"] = json!(2),
                    "short_final" => page.data["page"]["truncated"] = json!(false),
                    _ => unreachable!(),
                }
            }
            Ok(page)
        })
        .unwrap_err();
        assert_eq!(
            error.code,
            if matches!(fault_kind, "source" | "matched") {
                "records.hotfix_identity"
            } else {
                "records.hotfix_cursor"
            },
            "{fault_kind}"
        );
        assert_eq!(
            error.details["result"]["result"]["returned"], 1,
            "{fault_kind}"
        );
        assert_eq!(fixture.count_scans(), 1);
    }
}

#[test]
fn checkpoint_deserialization_preserves_go_struct_and_slice_merge() {
    let state: State = serde_json::from_str(
        r#"{
        "snapshot":"PIN-kept", "ſnapshot":null,
        "query":{"From":"CAP-kept","Filter":{"limit":1,"recordId":7}},
        "QUERY":{"Offline":true,"Filter":{"recordId":null,"latest":true}}, "query":null,
        "source":{"id":"CAP-kept","provenance":{"dataBuild":"build"}},
        "SOURCE":{"blob":{"bytes":-1},"provenance":{"kind":"hotfix-cache"}}, "source":null,
        "pages":["old-a","old-b"], "PAGES":["new-a"], "pages":[null,null],
        "matched":5,"MATCHED":null,"nextIndex":8,"nextIndex":null,
        "unknown":{"query":{"From":"CAP-wrong"}}
    }"#,
    )
    .unwrap();
    assert_eq!(state.snapshot, "PIN-kept");
    assert_eq!(state.source.id(), "CAP-kept");
    assert_eq!(state.source.data_build(), "build");
    assert_eq!(state.query.from, "CAP-kept");
    assert_eq!(state.query.filter.limit, 1);
    assert_eq!(state.query.filter.record_id, None);
    assert!(state.query.filter.latest && state.query.offline);
    assert_eq!(state.pages.values(), ["new-a", "old-b"]);
    assert_eq!(state.matched, 5);
    assert_eq!(state.next_index, None);
    let public = serde_json::to_value(state).unwrap();
    assert_eq!(public["source"]["blob"]["bytes"], -1);
    assert!(public.get("unknown").is_none());
    assert!(public["query"].get("from").is_none());
}

#[test]
fn checkpoint_null_empty_and_reused_page_arrays_are_distinct() {
    for (body, expected) in [
        (r#"{"pages":["old"],"pages":null}"#, Value::Null),
        (r#"{"pages":["old"],"pages":[]}"#, json!([])),
        (
            r#"{"pages":["old"],"pages":[],"pages":[null]}"#,
            json!([""]),
        ),
        (
            r#"{"pages":["old"],"pages":null,"pages":[null]}"#,
            json!([""]),
        ),
        (r#"{"pages":["old"],"pages":[null]}"#, json!(["old"])),
    ] {
        let state: State = serde_json::from_str(body).unwrap();
        assert_eq!(serde_json::to_value(state).unwrap()["pages"], expected);
    }
    for body in [
        r#"{"returned":-1}"#,
        r#"{"returned":4294967296}"#,
        r#"{"query":{"Filter":{"status":256}}}"#,
        r#"{"query":{"Filter":{"limit":"1"}}}"#,
        r#"{"pages":[1]}"#,
        "[]",
    ] {
        assert!(serde_json::from_str::<State>(body).is_err(), "{body}");
    }
}

#[test]
fn normalized_legacy_strings_preserve_replacement_and_raw_timestamp_rules() {
    let control = lycheedev_core::control::OperationControl::default();
    let raw = b"{\"snapshot\":\"\xff\",\"pages\":[\"\\ud800\"]}";
    let normalized = evidence::normalize_go_json_strings(raw, &control).unwrap();
    let state: State = serde_json::from_slice(&normalized).unwrap();
    assert_eq!(state.snapshot, "�");
    assert_eq!(state.pages.values(), ["�"]);
    let raw = br#"{"source":{"capturedAt":"2026-10-14T22:00:00\u005a"}}"#;
    let normalized = evidence::normalize_go_json_strings(raw, &control).unwrap();
    assert!(
        serde_json::from_slice::<State>(&normalized).is_err(),
        "Go time reader rejects a JSON-escaped timestamp byte"
    );
}

#[test]
fn manifest_output_budget_is_enforced_before_checkpoint_publication() {
    let fixture = Fixture::new();
    let budget = ledger(&fixture.inv);
    let state = State {
        snapshot: "x".repeat(MANIFEST_BYTES),
        ..Default::default()
    };
    assert_eq!(
        encoded_state(&state, &budget).unwrap_err().code,
        "records.hotfix_limit"
    );
    assert_eq!(fixture.count_scans(), 0);
}

#[test]
fn all_10000_page_dependencies_keep_one_live_reservation_and_cumulative_work() {
    use lycheedev_core::evidence::CaptureAccounting;
    let fixture = Fixture::new();
    let budget = ledger(&fixture.inv);
    let meter = ScanCaptureAccounting::new(&budget);
    let length = 1 << 20;
    for _ in 0..=MAX_PAGES {
        meter.admit_dependency(length).unwrap();
        assert_eq!(
            budget.snapshot().unwrap().retained_bytes,
            (length * 24 + 4096) as u64
        );
    }
    let used = budget.snapshot().unwrap();
    assert_eq!(used.metadata_bytes, ((MAX_PAGES + 1) * length) as u64);
    assert_eq!(used.decode_work, ((MAX_PAGES + 1) * length * 8) as u64);
    drop(meter);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    assert_eq!(
        budget.snapshot().unwrap().metadata_bytes,
        used.metadata_bytes
    );
    assert_eq!(budget.snapshot().unwrap().decode_work, used.decode_work);
}

#[test]
fn dependency_peak_metadata_and_work_limits_fail_independently() {
    use lycheedev_core::evidence::CaptureAccounting;
    let fixture = Fixture::new();
    for limits in [
        Cost {
            retained_bytes: 24 * 1024 + 4095,
            ..Cost::defaults()
        },
        Cost {
            metadata_bytes: 1023,
            ..Cost::defaults()
        },
        Cost {
            decode_work: 8191,
            ..Cost::defaults()
        },
    ] {
        let budget = Budget::controlled(
            limits,
            fixture.inv.control.deadline(),
            fixture.inv.control.clone(),
        );
        let meter = ScanCaptureAccounting::new(&budget);
        assert_eq!(
            meter.admit_dependency(1024).unwrap_err().code,
            "records.query_resource_budget"
        );
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    }
    let budget = ledger(&fixture.inv);
    let meter = ScanCaptureAccounting::new(&budget);
    let existing = budget.reserve_retained((512 << 20) - 4096).unwrap();
    assert_eq!(
        meter.admit_dependency(1).unwrap_err().code,
        "records.query_resource_budget"
    );
    drop(existing);
    meter.admit_dependency(1).unwrap();
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 4120);
}

#[test]
fn cumulative_scan_page_limit_is_checked_before_another_page_is_inspected() {
    let mut state = State::default();
    for _ in 0..MAX_PAGES - 1 {
        state.pages.push("CAP-previous".into());
    }
    admit_page(&state).unwrap();
    state.pages.push("CAP-last".into());
    assert_eq!(admit_page(&state).unwrap_err().code, "records.hotfix_limit");
}

#[test]
fn signed_request_limits_keep_intermediate_negatives_and_reject_intermediate_overflow() {
    let state: State = serde_json::from_str(
        r#"{"query":{
        "MaxBytes":-1,"maxbytes":null,"MAXBYTES":1048576,
        "Filter":{"limit":-1,"LIMIT":null,"limit":1}
    }}"#,
    )
    .unwrap();
    assert_eq!(state.query.max_bytes, 1048576);
    assert_eq!(state.query.filter.limit, 1);
    let negative: State =
        serde_json::from_str(r#"{"query":{"MaxBytes":-1,"Filter":{"limit":-1}}}"#).unwrap();
    assert_eq!(negative.query.max_bytes, -1);
    assert_eq!(negative.query.filter.limit, -1);
    for overflow in [
        "9223372036854775808",
        "-9223372036854775809",
        "18446744073709551615",
    ] {
        for body in [
            format!(r#"{{"query":{{"MaxBytes":{overflow},"MaxBytes":1048576}}}}"#),
            format!(r#"{{"query":{{"Filter":{{"limit":{overflow},"limit":1}}}}}}"#),
        ] {
            assert!(serde_json::from_str::<State>(&body).is_err(), "{body}");
        }
    }
}

#[test]
fn all_four_signed_checkpoint_fields_accept_lexical_negative_zero() {
    let state: State = serde_json::from_str(
        r#"{
        "query":{"MaxBytes":-0,"Filter":{"limit":-0,"push":-0}},
        "source":{"blob":{"bytes":-0}}
    }"#,
    )
    .unwrap();
    assert_eq!(state.query.max_bytes, 0);
    assert_eq!(state.query.filter.limit, 0);
    assert_eq!(state.query.filter.push, Some(0));
    let public = serde_json::to_value(state).unwrap();
    assert_eq!(public["source"]["blob"]["bytes"], 0);
    assert_eq!(public["query"]["MaxBytes"], 0);
    assert_eq!(public["query"]["Filter"]["limit"], 0);
    assert_eq!(public["query"]["Filter"]["push"], 0);
    let nulls: State = serde_json::from_str(r#"{
        "query":{"MaxBytes":-0,"MaxBytes":null,"Filter":{"limit":-0,"limit":null,"push":-0,"push":null}},
        "source":{"blob":{"bytes":-0,"bytes":null}}
    }"#).unwrap();
    assert_eq!(nulls.query.max_bytes, 0);
    assert_eq!(nulls.query.filter.limit, 0);
    assert_eq!(nulls.query.filter.push, None);
    assert_eq!(
        serde_json::to_value(nulls).unwrap()["source"]["blob"]["bytes"],
        0
    );
}

#[test]
fn signed_checkpoint_fields_reject_decimal_exponent_strings_and_forged_numbers() {
    for token in [
        "1.0",
        "1e0",
        "-0.0",
        "-0e0",
        "1E+0",
        r#""1""#,
        "{}",
        "[]",
        "true",
        r#"{"$serde_json::private::Number":"1"}"#,
    ] {
        for body in [
            format!(r#"{{"query":{{"MaxBytes":{token},"MaxBytes":1}}}}"#),
            format!(r#"{{"query":{{"Filter":{{"limit":{token},"limit":1}}}}}}"#),
            format!(r#"{{"query":{{"Filter":{{"push":{token},"push":1}}}}}}"#),
            format!(r#"{{"source":{{"blob":{{"bytes":{token},"bytes":1}}}}}}"#),
        ] {
            assert!(serde_json::from_str::<State>(&body).is_err(), "{body}");
        }
    }
}

#[test]
fn signed_wire_ranges_match_go_int64_and_int32_before_duplicate_overwrite() {
    for bound in [i64::MIN, i64::MAX] {
        let raw = format!(
            r#"{{"query":{{"MaxBytes":{bound},"Filter":{{"limit":{bound}}}}},"source":{{"blob":{{"bytes":{bound}}}}}}}"#
        );
        let state: State = serde_json::from_str(&raw).unwrap();
        assert_eq!(state.query.max_bytes, bound);
        assert_eq!(state.query.filter.limit, bound);
        assert_eq!(
            serde_json::to_value(state).unwrap()["source"]["blob"]["bytes"],
            bound
        );
    }
    for bound in [i32::MIN, i32::MAX] {
        let raw = format!(r#"{{"query":{{"Filter":{{"push":{bound}}}}}}}"#);
        let state: State = serde_json::from_str(&raw).unwrap();
        assert_eq!(state.query.filter.push, Some(bound));
    }
    for overflow in ["-2147483649", "2147483648", "9223372036854775808"] {
        let raw = format!(r#"{{"query":{{"Filter":{{"push":{overflow},"push":0}}}}}}"#);
        assert!(serde_json::from_str::<State>(&raw).is_err(), "{raw}");
    }
    for overflow in ["-9223372036854775809", "9223372036854775808"] {
        let raw = format!(r#"{{"source":{{"blob":{{"bytes":{overflow},"bytes":0}}}}}}"#);
        assert!(serde_json::from_str::<State>(&raw).is_err(), "{raw}");
    }
}
