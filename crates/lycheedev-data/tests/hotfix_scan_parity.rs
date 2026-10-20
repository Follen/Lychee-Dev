//! Fixed Go 3.2.0 hotfix_scan_test.go / hotfix_scan_retention_test.go.
//! Offline synthetic XFTH inputs; no game or mutable network input.
#[path = "support/data_cache.rs"]
mod data_cache;
use lycheedev_core::{
    Invocation, Outcome, evidence,
    metadata::Metadata,
    selection::{self, DataPin, SelectionSpec},
    storage, workspace,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::PathBuf};
const BUILD: &str = "12.1.0.69875";
struct Fixture {
    root: PathBuf,
    home: PathBuf,
    file: PathBuf,
    pin: String,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lychee-hotfix-scan-{}-{}",
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
                    full_build: BUILD.into(),
                    build_config: "b".repeat(32),
                    cdn_config: "c".repeat(32),
                    definition_commit: "a".repeat(40),
                }),
                ..Default::default()
            },
        )
        .unwrap()
        .id;
        let file = root.join("DBCache.bin");
        let fixture = Self {
            root,
            home,
            file,
            pin,
        };
        fixture.write(&[(1, 2, -3, b""), (1, 1, -2, b""), (3, 1, -1, b"")]);
        fixture
    }
    fn write(&self, entries: &[(u32, u8, i32, &[u8])]) {
        let mut raw = b"XFTH".to_vec();
        raw.extend(9u32.to_le_bytes());
        raw.extend(69875u32.to_le_bytes());
        raw.resize(44, 0);
        for &(id, status, push, payload) in entries {
            let mut header = [0u8; 32];
            header[..4].copy_from_slice(b"XFTH");
            header[4..8].copy_from_slice(&3u32.to_le_bytes());
            header[8..12].copy_from_slice(&push.to_le_bytes());
            header[16..20].copy_from_slice(&42u32.to_le_bytes());
            header[20..24].copy_from_slice(&id.to_le_bytes());
            header[24..28].copy_from_slice(&(payload.len() as u32).to_le_bytes());
            header[28] = status;
            raw.extend(header);
            raw.extend(payload);
        }
        fs::write(&self.file, raw).unwrap();
    }
    fn inv(&self) -> Invocation {
        Invocation {
            command: "data hotfix".into(),
            positional: None,
            home: self.home.clone(),
            cwd: self.root.clone(),
            control: Default::default(),
            flags: BTreeMap::from([
                ("--source".into(), "dbcache".into()),
                ("--scan".into(), "true".into()),
                ("--snapshot".into(), self.pin.clone()),
                ("--dbcache".into(), self.file.to_string_lossy().into_owned()),
                ("--limit".into(), "1".into()),
                ("--max-bytes".into(), (1 << 20).to_string()),
                ("--max-pages".into(), "1".into()),
                ("--offline".into(), "true".into()),
            ]),
        }
    }
    fn resume(&self, out: &Outcome) -> Invocation {
        let mut inv = self.inv();
        inv.flags.remove("--dbcache");
        inv.flags
            .insert("--snapshot".into(), string(&out.data["result"]["snapshot"]));
        inv.flags
            .insert("--from".into(), string(&out.data["result"]["source"]["id"]));
        inv.flags
            .insert("--cursor".into(), string(&out.data["capture"]["id"]));
        inv
    }
    fn count(&self) -> usize {
        Metadata::open(&self.home, false)
            .unwrap()
            .list::<Value>("capture/", 1000)
            .unwrap()
            .len()
    }
    fn named(&self) {
        let commit = "a".repeat(40);
        let manifest = serde_json::to_vec(
            &json!([{"tableName":"Example","tableHash":"0000002A","db2FileDataID":1}]),
        )
        .unwrap();
        for (path, raw) in [("manifest.json", manifest.as_slice()), ("definitions/Example.dbd", b"COLUMNS\nint ID\nstring Name\n\nBUILD 12.1.0.69875\n$noninline,id$ID<u32>\nName\n".as_slice())] {
            let url = format!("https://raw.githubusercontent.com/wowdev/WoWDBDefs/{commit}/{path}");
            let blob = evidence::publish_blob(&self.home, raw, 4 << 20).unwrap();
            data_cache::Cache::new(&self.home).put(&format!("definition/{}", storage::sha256(url.as_bytes())), &json!({"url":url,"blob":blob}), None).unwrap();
        }
    }
    fn legacy(&self, original: &Outcome, missing_page: bool) -> Value {
        let reference = &original.data["capture"];
        let raw = evidence::read_blob(&self.home, &reference["blob"], 4 << 20).unwrap();
        let all = Metadata::open(&self.home, false)
            .unwrap()
            .list::<Value>("capture/", 1000)
            .unwrap();
        for capture in all {
            if capture["provenance"]["kind"] == "hotfix-scan" {
                evidence::remove_capture(&self.home, capture["id"].as_str().unwrap()).unwrap();
            }
        }
        if missing_page {
            evidence::remove_capture(
                &self.home,
                original.data["result"]["pages"][0].as_str().unwrap(),
            )
            .unwrap();
        }
        self.legacy_body(&raw, &original.data["result"], original.complete)
    }
    fn legacy_body(&self, raw: &[u8], state: &Value, complete: bool) -> Value {
        evidence::commit_capture(
            &self.home,
            raw,
            "application/json",
            json!({"kind":"hotfix-scan",
            "locator":format!("capture:{}", state["source"]["id"].as_str().unwrap()),
            "snapshot":state["snapshot"], "dataBuild":BUILD}),
            complete,
            !complete,
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn string(value: &Value) -> String {
    value.as_str().unwrap().to_owned()
}
fn run(inv: &Invocation) -> Outcome {
    lycheedev_data::migration_oracle::dispatch(inv).unwrap()
}

#[test]
fn scan_resumes_immutable_source_and_replays_complete_without_new_captures() {
    let fixture = Fixture::new();
    let first = run(&fixture.inv());
    let state = &first.data["result"];
    assert_eq!(state["returned"], 1);
    assert_eq!(state["matched"], 3);
    assert_eq!(state["raw"], 1);
    assert_eq!(state["complete"], false);
    assert_eq!(state["truncated"], true);
    assert_eq!(state["nextIndex"], 0);
    assert_eq!(state["query"]["File"], "");
    assert_eq!(state["query"]["From"], state["source"]["id"]);
    assert_eq!(state["query"]["Filter"]["build"], 0);
    assert_eq!(first.warnings.len(), 1);
    assert_eq!(first.selection, json!({"snapshot":state["snapshot"]}));
    fs::write(&fixture.file, b"changed original file").unwrap();
    let mut resume = fixture.resume(&first);
    resume.flags.insert("--max-pages".into(), "3".into());
    resume.flags.insert("--offline".into(), "false".into());
    let done = run(&resume);
    assert_eq!(done.data["result"]["returned"], 3);
    assert_eq!(done.data["result"]["pages"].as_array().unwrap().len(), 3);
    assert!(done.complete);
    assert!(done.data.get("resume").is_none());
    assert!(done.data["result"].get("nextIndex").is_none());
    assert_eq!(
        done.data["result"]["query"]["Offline"], true,
        "saved request is immutable across offline changes"
    );
    let count = fixture.count();
    let replay = run(&fixture.resume(&done));
    assert_eq!(replay.data, done.data);
    assert_eq!(fixture.count(), count);
    let replay_old = run(&resume);
    assert_eq!(
        replay_old.data["result"]["returned"], 3,
        "replaying old cursor does not double count"
    );
}

#[test]
fn scan_checkpoints_protect_shared_pages_then_release_only_their_own_references() {
    let fixture = Fixture::new();
    let first = run(&fixture.inv());
    let second = run(&fixture.resume(&first));
    let done = run(&fixture.resume(&second));
    for out in [&first, &second, &done] {
        let mut dependencies = vec![out.data["result"]["source"]["id"].clone()];
        dependencies.extend(
            out.data["result"]["pages"]
                .as_array()
                .unwrap()
                .iter()
                .cloned(),
        );
        assert_eq!(
            out.data["capture"]["dependencies"],
            Value::Array(dependencies)
        );
    }
    let page = string(&first.data["result"]["pages"][0]);
    let old = evidence::read_capture(&fixture.home, &page).unwrap();
    let bytes = evidence::read_blob(&fixture.home, &old["blob"], 32 << 20).unwrap();
    let source = string(&first.data["result"]["source"]["id"]);
    for id in [&source, &page] {
        assert_eq!(
            evidence::remove_capture(&fixture.home, id)
                .unwrap_err()
                .code,
            "evidence.capture_referenced"
        );
    }
    for out in [&done, &first] {
        evidence::remove_capture(&fixture.home, out.data["capture"]["id"].as_str().unwrap())
            .unwrap();
        assert_eq!(
            evidence::remove_capture(&fixture.home, &page)
                .unwrap_err()
                .code,
            "evidence.capture_referenced"
        );
    }
    assert_eq!(evidence::read_capture(&fixture.home, &page).unwrap(), old);
    assert_eq!(
        evidence::read_blob(&fixture.home, &old["blob"], 32 << 20).unwrap(),
        bytes
    );
    evidence::remove_capture(
        &fixture.home,
        second.data["capture"]["id"].as_str().unwrap(),
    )
    .unwrap();
    for id in done.data["result"]["pages"].as_array().unwrap() {
        evidence::remove_capture(&fixture.home, id.as_str().unwrap()).unwrap();
    }
    evidence::remove_capture(&fixture.home, &source).unwrap();
}

#[test]
fn scan_query_identity_freezes_every_filter_and_the_derived_snapshot() {
    let fixture = Fixture::new();
    let first = run(&fixture.inv());
    let original = fixture.resume(&first);
    for (flag, value) in [
        ("--record", "42"),
        ("--table-hash", "0000002A"),
        ("--table", "Example"),
        ("--push", "-3"),
        ("--region-id", "3"),
        ("--status", "2"),
        ("--latest", "true"),
        ("--limit", "2"),
        ("--max-bytes", "1024"),
    ] {
        let mut changed = original.clone();
        changed.flags.insert(flag.into(), value.into());
        let before = fixture.count();
        let error = lycheedev_data::migration_oracle::dispatch(&changed).unwrap_err();
        assert_eq!(error.code, "records.hotfix_identity", "{flag}");
        assert!(error.details.get("result").is_none());
        assert_eq!(fixture.count(), before);
    }
    let mut wrong_pin = original;
    wrong_pin
        .flags
        .insert("--snapshot".into(), fixture.pin.clone());
    assert_eq!(
        lycheedev_data::migration_oracle::dispatch(&wrong_pin).unwrap_err().code,
        "records.hotfix_identity"
    );
    let mut file_resume = fixture.inv();
    file_resume
        .flags
        .insert("--cursor".into(), string(&first.data["capture"]["id"]));
    assert_eq!(
        lycheedev_data::migration_oracle::dispatch(&file_resume).unwrap_err().code,
        "records.hotfix_identity"
    );
}

#[test]
fn named_scan_counts_payload_states_without_losing_raw_metadata() {
    let fixture = Fixture::new();
    fixture.named();
    fixture.write(&[
        (42, 1, -3, b"hello\0"),
        (42, 2, -2, b"bad payload"),
        (9, 1, -1, b""),
    ]);
    let mut inv = fixture.inv();
    inv.flags.insert("--table".into(), "Example".into());
    inv.flags.insert("--max-pages".into(), "0".into());
    let out = run(&inv);
    let state = &out.data["result"];
    assert_eq!(state["decoded"], 1);
    assert_eq!(state["notValid"], 1);
    assert_eq!(state["noPayload"], 1);
    assert_eq!(state["raw"], 0);
    assert_eq!(state["query"]["Table"], "Example");
    assert!(state["query"]["Filter"].get("tableHash").is_none());
    for (position, id) in state["pages"].as_array().unwrap().iter().enumerate() {
        let capture = evidence::read_capture(&fixture.home, id.as_str().unwrap()).unwrap();
        let body: Value = serde_json::from_slice(
            &evidence::read_blob(&fixture.home, &capture["blob"], 32 << 20).unwrap(),
        )
        .unwrap();
        let entry = &body["page"]["entries"][0];
        assert_eq!(entry["index"], position);
        if position == 0 {
            assert_eq!(entry["fields"], json!({"ID":42,"Name":"hello"}));
        } else {
            assert!(entry.get("fields").is_none());
        }
    }
}

#[test]
fn scan_empty_match_is_one_complete_empty_page_and_latest_keeps_negative_pushes() {
    let fixture = Fixture::new();
    let mut inv = fixture.inv();
    inv.flags.insert("--record".into(), "999".into());
    let out = run(&inv);
    let state = &out.data["result"];
    assert!(out.complete);
    assert_eq!(state["returned"], 0);
    assert_eq!(state["matched"], 0);
    assert_eq!(state["pages"].as_array().unwrap().len(), 1);
    assert!(state.get("nextIndex").is_none());
    let capture =
        evidence::read_capture(&fixture.home, state["pages"][0].as_str().unwrap()).unwrap();
    let body: Value = serde_json::from_slice(
        &evidence::read_blob(&fixture.home, &capture["blob"], 32 << 20).unwrap(),
    )
    .unwrap();
    assert_eq!(body["page"]["entries"], json!([]));
    inv.flags.remove("--record");
    inv.flags.insert("--latest".into(), "true".into());
    let latest = run(&inv);
    assert_eq!(latest.data["result"]["matched"], 1);
    assert_eq!(latest.data["result"]["returned"], 1);
}

#[test]
fn scan_argument_limits_keep_query_errors_and_scan_errors_distinct() {
    let fixture = Fixture::new();
    for (flag, value, exit) in [
        ("--max-pages", "-1", 3),
        ("--max-pages", "1001", 3),
        ("--limit", "0", 2),
        ("--limit", "201", 2),
        ("--max-bytes", "0", 2),
        ("--max-bytes", "536870913", 2),
        ("--after-index", "0", 2),
    ] {
        let mut inv = fixture.inv();
        inv.flags.insert(flag.into(), value.into());
        assert_eq!(
            lycheedev_data::migration_oracle::dispatch(&inv).unwrap_err().exit_code,
            exit,
            "{flag}={value}"
        );
    }
    let first = run(&fixture.inv());
    let mut inv = fixture.resume(&first);
    inv.flags.insert("--after-index".into(), "0".into());
    let error = lycheedev_data::migration_oracle::dispatch(&inv).unwrap_err();
    assert_eq!(error.code, "records.hotfix_limit");
    assert_eq!(error.exit_code, 3);
    for flag in [
        "--raidbots",
        "--product",
        "--build",
        "--region",
        "--locale",
        "--search",
        "--to",
        "--page",
        "--max-requests",
    ] {
        let mut inv = fixture.inv();
        inv.flags.insert(flag.into(), "1".into());
        assert_eq!(
            lycheedev_data::migration_oracle::dispatch(&inv).unwrap_err().exit_code,
            2,
            "{flag}"
        );
    }
    assert_eq!(
        fixture.count(),
        3,
        "only the intentional first page published evidence"
    );
}

#[test]
fn legacy_pending_and_complete_checkpoints_preserve_bytes_and_retain_pages() {
    for complete in [false, true] {
        let fixture = Fixture::new();
        let mut inv = fixture.inv();
        if complete {
            inv.flags.insert("--max-pages".into(), "3".into());
        }
        let original = run(&inv);
        let legacy = fixture.legacy(&original, false);
        let old = evidence::read_blob(&fixture.home, &legacy["blob"], 4 << 20).unwrap();
        for id in original.data["result"]["pages"].as_array().unwrap() {
            assert_eq!(
                evidence::remove_capture(&fixture.home, id.as_str().unwrap())
                    .unwrap_err()
                    .code,
                "evidence.capture_referenced"
            );
        }
        let mut resume = fixture.resume(&original);
        resume
            .flags
            .insert("--cursor".into(), string(&legacy["id"]));
        resume.flags.insert("--max-pages".into(), "3".into());
        let done = run(&resume);
        assert!(done.complete);
        assert_eq!(done.data["result"]["returned"], 3);
        if complete {
            assert_eq!(done.data["capture"], legacy);
        }
        assert_eq!(
            evidence::read_blob(&fixture.home, &legacy["blob"], 4 << 20).unwrap(),
            old
        );
    }
}

#[test]
fn legacy_missing_page_never_reports_complete_or_loses_original_resume_identity() {
    for complete in [false, true] {
        let fixture = Fixture::new();
        let mut inv = fixture.inv();
        if complete {
            inv.flags.insert("--max-pages".into(), "3".into());
        }
        let original = run(&inv);
        let legacy = fixture.legacy(&original, true);
        let mut resume = fixture.resume(&original);
        resume
            .flags
            .insert("--cursor".into(), string(&legacy["id"]));
        let before = fixture.count();
        let error = lycheedev_data::migration_oracle::dispatch(&resume).unwrap_err();
        assert_eq!(error.code, "vault.record_missing");
        assert_eq!(error.details["result"]["resume"], legacy["id"]);
        assert_eq!(error.details["result"]["capture"], legacy);
        assert_eq!(error.details["result"]["result"], original.data["result"]);
        assert_eq!(
            error.details["__commandContext"]["snapshot"],
            original.data["result"]["snapshot"]
        );
        assert_eq!(fixture.count(), before);
    }
}

#[test]
fn legacy_wire_case_duplicate_null_merge_and_unknown_fields_resume_like_go() {
    let fixture = Fixture::new();
    let original = run(&fixture.inv());
    let mut text = serde_json::to_string(&original.data["result"]).unwrap();
    text.pop();
    text.push_str(",\"QUERY\":{\"Offline\":false,\"Filter\":{\"Latest\":true,\"latest\":false,\"limit\":null}},\"query\":null,\"source\":{},\"SOURCE\":null,\"matched\":null,\"UNKNOWN\":{\"pages\":[7]}}");
    let legacy = fixture.legacy_body(text.as_bytes(), &original.data["result"], false);
    let mut resume = fixture.resume(&original);
    resume
        .flags
        .insert("--cursor".into(), string(&legacy["id"]));
    resume.flags.insert("--max-pages".into(), "3".into());
    let done = run(&resume);
    assert!(done.complete);
    assert_eq!(done.data["result"]["returned"], 3);
    assert_eq!(done.data["result"]["query"]["Offline"], false);
    assert_eq!(
        done.data["result"]["source"],
        original.data["result"]["source"]
    );
}

#[test]
fn scan_rejects_other_or_missing_sources_before_unsupported_dispatch() {
    let fixture = Fixture::new();
    for source in [None, Some("wago"), Some("raidbots"), Some("other")] {
        let mut inv = fixture.inv();
        inv.flags.remove("--source");
        if let Some(source) = source {
            inv.flags.insert("--source".into(), source.into());
        }
        let error = lycheedev_data::migration_oracle::dispatch(&inv).unwrap_err();
        assert_eq!(error.code, "command.invalid_arguments");
        assert_eq!(error.message, "--scan requires --source dbcache");
        assert_eq!(error.exit_code, 2);
    }
    assert_eq!(fixture.count(), 0);
}

#[test]
fn legacy_signed_duplicate_limits_and_negative_zero_resume_through_real_storage() {
    for intermediate in ["-1", "-0"] {
        let fixture = Fixture::new();
        let original = run(&fixture.inv());
        let mut text = serde_json::to_string(&original.data["result"]).unwrap();
        text.pop();
        let source_bytes = original.data["result"]["source"]["blob"]["bytes"]
            .as_u64()
            .unwrap();
        text.push_str(&format!(r#", "QUERY":{{"MaxBytes":{intermediate},"MaxBytes":null,"MaxBytes":1048576,"Filter":{{"limit":{intermediate},"limit":null,"limit":1,"push":-0,"push":null}}}},"SOURCE":{{"blob":{{"bytes":-0,"bytes":null,"bytes":{source_bytes}}}}}}}"#));
        let legacy = fixture.legacy_body(text.as_bytes(), &original.data["result"], false);
        let mut resume = fixture.resume(&original);
        resume
            .flags
            .insert("--cursor".into(), string(&legacy["id"]));
        resume.flags.insert("--max-pages".into(), "3".into());
        let done = run(&resume);
        assert!(done.complete);
        assert_eq!(done.data["result"]["returned"], 3);
        assert_eq!(
            done.data["result"]["query"],
            original.data["result"]["query"]
        );
        assert_eq!(
            done.data["result"]["source"],
            original.data["result"]["source"]
        );
    }
}
