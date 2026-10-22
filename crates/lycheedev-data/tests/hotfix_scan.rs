//! Retained hotfix-scan wire regressions (scan and retention coverage).
//! Offline synthetic XFTH inputs; no game or mutable network input.
#[path = "support/data_cache.rs"]
mod data_cache;
use lycheedev_core::{
    Outcome, evidence,
    metadata::Metadata,
    selection::{self, DataPin, SelectionSpec},
    storage, workspace,
};
use serde_json::{Value, json};
use lycheedev_data::{hotfix::{self, Request, LocalInput, LocalRequest, ScanRequest, CacheFilter},request::DataContext};
use std::{fs, path::PathBuf};
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
    fn context(&self) -> DataContext {DataContext {home:self.home.clone(),cwd:self.root.clone(),control:Default::default(),source:Default::default()}}
    fn request(&self) -> ScanRequest {
        ScanRequest {page:LocalRequest {snapshot:self.pin.clone(),input:LocalInput::File(self.file.clone()),table:None,offline:true,max_bytes:1<<20,
            filter:CacheFilter {limit:1,..Default::default()}},cursor:String::new(),max_pages:1}
    }
    fn resume(&self, out: &Outcome) -> ScanRequest {
        let mut request=self.request();
        request.page.snapshot=string(&out.data["result"]["snapshot"]);
        request.page.input=LocalInput::Capture(string(&out.data["result"]["source"]["id"]));
        request.cursor=string(&out.data["capture"]["id"]);request
    }
    fn run(&self, request: &ScanRequest) -> lycheedev_core::Result<Outcome> {
        hotfix::dispatch(&self.context(),&Request::Scan(request.clone()))
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
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn string(value: &Value) -> String {
    value.as_str().unwrap().to_owned()
}
#[test]
fn scan_resumes_immutable_source_and_replays_complete_without_new_captures() {
    let fixture = Fixture::new();
    let first = fixture.run(&fixture.request()).unwrap();
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
    resume.max_pages = 3;
    resume.page.offline = false;
    let done = fixture.run(&resume).unwrap();
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
    let replay = fixture.run(&fixture.resume(&done)).unwrap();
    assert_eq!(replay.data, done.data);
    assert_eq!(fixture.count(), count);
    let replay_old = fixture.run(&resume).unwrap();
    assert_eq!(
        replay_old.data["result"]["returned"], 3,
        "replaying old cursor does not double count"
    );
}

#[test]
fn scan_checkpoints_protect_shared_pages_then_release_only_their_own_references() {
    let fixture = Fixture::new();
    let first = fixture.run(&fixture.request()).unwrap();
    let second = fixture.run(&fixture.resume(&first)).unwrap();
    let done = fixture.run(&fixture.resume(&second)).unwrap();
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
    let first = fixture.run(&fixture.request()).unwrap();
    let original = fixture.resume(&first);
    for field in ["record","table_hash","table","push","region","status","latest","limit","max_bytes"] {
        let mut changed=original.clone();
        match field {"record"=>changed.page.filter.record_id=Some(42),"table_hash"=>changed.page.filter.table_hash=Some(42),
            "table"=>changed.page.table=Some("Example".into()),"push"=>changed.page.filter.push=Some(-3),"region"=>changed.page.filter.region=Some(3),
            "status"=>changed.page.filter.status=Some(2),"latest"=>changed.page.filter.latest=true,"limit"=>changed.page.filter.limit=2,
            "max_bytes"=>changed.page.max_bytes=1024,_=>unreachable!()};
        let before = fixture.count();
        let error = fixture.run(&changed).unwrap_err();
        assert_eq!(error.code, "records.hotfix_identity", "{field}");
        assert!(error.details.get("result").is_none());
        assert_eq!(fixture.count(), before);
    }
    let mut wrong_pin = original;
    wrong_pin.page.snapshot = fixture.pin.clone();
    assert_eq!(
        fixture.run(&wrong_pin).unwrap_err().code,
        "records.hotfix_identity"
    );
    let mut file_resume = fixture.request();
    file_resume.cursor = string(&first.data["capture"]["id"]);
    assert_eq!(
        fixture.run(&file_resume).unwrap_err().code,
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
    let mut request = fixture.request();
    request.page.table = Some("Example".into());
    request.max_pages = 100;
    let out = fixture.run(&request).unwrap();
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
    let mut request = fixture.request();
    request.page.filter.record_id = Some(999);
    let out = fixture.run(&request).unwrap();
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
    request.page.filter.record_id = None;
    request.page.filter.latest = true;
    let latest = fixture.run(&request).unwrap();
    assert_eq!(latest.data["result"]["matched"], 1);
    assert_eq!(latest.data["result"]["returned"], 1);
}

#[test]
fn scan_after_index_request_is_rejected_without_another_checkpoint() {
    let fixture=Fixture::new();let first=fixture.run(&fixture.request()).unwrap();
    let mut request=fixture.resume(&first);request.page.filter.after_index=Some(0);
    let error=fixture.run(&request).unwrap_err();
    assert_eq!(error.code,"records.hotfix_limit");assert_eq!(error.exit_code,3);
    assert_eq!(fixture.count(),3,"only the intentional first page published evidence");
}
