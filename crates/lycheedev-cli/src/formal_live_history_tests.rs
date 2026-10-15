//! NOT_RUN. Actual formal entry + workspace/SQLite/blob path, no native stub
//! for historical reads. Go entry.go:705–764; outcome.go:99–179; archive.go.
use super::*;
use lycheedev_core::{evidence, metadata::Metadata};
use lycheedev_live::go_mailbox_v1::admission::ConnectionTarget;
fn dispatch_history(inv: &Invocation) -> Response {
    crate::dispatch_formal_entry_with(inv, |inv| {
        dispatch_with::<Fake>(inv, |_| {
            panic!("historical route constructed native services")
        })
    })
    .expect("formal entry")
}
fn work() -> Value {
    json!({"schema":"lycheedev.work.v1","operationId":"OP-history","generation":1,"intent":{"kind":"probe","resource":"window/1/2/3","snapshot":"PIN-test","session":"session","goal":"finished","request":{"schema":"lycheedev.report-intent.v1","budgetSeconds":5,"expected":{"Kind":"reported","Release":"3.2.0","SessionNonce":"session","RequestID":"request","Character":"paladin","Realm":"realm","Product":"retail","Build":"12.1.0.69875"}}},"stage":"prepared","status":"pending","observation":null})
}
fn save(inv: &Invocation, value: &Value) {
    Metadata::open(&inv.home, true)
        .unwrap()
        .create("work/OP-history", value)
        .unwrap();
}
fn envelope(response: &Response) -> Value {
    let mut bytes = vec![];
    render(&mut bytes, "json", response).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
fn verified(inv: &Invocation, body: &[u8]) -> Value {
    let mut a = 1u32;
    let mut b = 0u32;
    for byte in body {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    let receipt=serde_json::to_vec(&json!({"schema":"lycheedev.signal.v1","release":"3.2.0","kind":"reported","sessionNonce":"session","requestId":"request","character":"paladin","realm":"realm","product":"retail","build":"12.1.0.69875","sequence":1,"inputReady":false,"reportBytes":body.len(),"reportAdler32":format!("{:08x}",(b<<16)|a)})).unwrap();
    let capture = |kind: &str, bytes: &[u8]| {
        evidence::commit_capture(&inv.home,bytes,"application/json",json!({"kind":kind,"locator":"request","snapshot":"PIN-test","operationId":"OP-history","dataBuild":"12.1.0.69875","session":"session"}),true,false).unwrap()["id"].as_str().unwrap().to_string()
    };
    let mut value = work();
    value["stage"] = json!("verified");
    value["status"] = json!("running");
    value["observation"] =
        json!({"bodyId":capture("game-report",body),"receiptId":capture("game-receipt",&receipt)});
    value
}
#[test]
fn historical_op_real_entry_reads_pending_without_native_or_replay() {
    let f = Fixture::new();
    let inv = invocation(&["live", "status", "OP-history"], &f.0);
    lycheedev_core::workspace::initialize(&inv.home).unwrap();
    save(&inv, &work());
    let response = dispatch_history(&inv);
    assert_eq!(response.exit_code(), 0);
    let value = envelope(&response);
    assert_eq!(value["result"]["operationId"], "OP-history");
    assert_eq!(value["result"]["cleanup"], "pending");
    assert_eq!(value["result"]["report"]["state"], "unavailable");
    assert_eq!(
        value["result"]["nextAction"]["args"],
        json!(["live", "abandon", "OP-history"])
    );
    assert!(!f.0.join(".lycheedev").exists());
}
#[test]
fn historical_op_real_entry_errors_clear_typed_library_outcome() {
    for malformed in [false, true] {
        let f = Fixture::new();
        let inv = invocation(&["live", "status", "OP-history"], &f.0);
        lycheedev_core::workspace::initialize(&inv.home).unwrap();
        if malformed {
            let mut w = work();
            w["observation"] = json!(17);
            save(&inv, &w);
        }
        let read =
            lycheedev_live::history::inspect_operation(&inv.home, "OP-history", &inv.control);
        assert!(read.error.is_some());
        assert_eq!(
            read.outcome.operation_id,
            if malformed { "OP-history" } else { "" }
        );
        let response = dispatch_history(&inv);
        assert_eq!(response.exit_code(), if malformed { 5 } else { 2 });
        let value = envelope(&response);
        assert_eq!(value["operationId"], "OP-history");
        assert!(value["result"].is_null());
    }
}
#[test]
fn historical_op_real_blob_business_error_retains_library_raw_but_cli_clears_it() {
    let f = Fixture::new();
    let inv = invocation(&["live", "status", "OP-history"], &f.0);
    lycheedev_core::workspace::initialize(&inv.home).unwrap();
    let body=br#"{ "probeStatus":"completed", "acceptedBudgetSeconds":6, "wide":9007199254740993, "first":1, "second":2 }"#;
    save(&inv, &verified(&inv, body));
    let read = lycheedev_live::history::inspect_operation(&inv.home, "OP-history", &inv.control);
    assert_eq!(
        read.error.as_ref().unwrap().message,
        "live.execution_budget_mismatch"
    );
    assert_eq!(read.outcome.report.state, "verified");
    let bytes = serde_json::to_vec(&read.outcome).unwrap();
    assert!(bytes.windows(body.len()).any(|w| w == body));
    assert!(!read.outcome.complete);
    let response = dispatch_history(&inv);
    assert_eq!(response.exit_code(), 5);
    let value = envelope(&response);
    assert!(value["result"].is_null());
    assert_eq!(value["error"]["message"], "live.execution_budget_mismatch");
}
#[test]
fn historical_op_real_blob_display_error_preserves_library_state_and_clears_cli_result() {
    let f = Fixture::new();
    let inv = invocation(&["live", "status", "OP-history"], &f.0);
    lycheedev_core::workspace::initialize(&inv.home).unwrap();
    let body = br#"{"probeStatus":"completed","acceptedBudgetSeconds":5,"result":{"passed":true}}"#;
    save(&inv, &verified(&inv, body));
    Metadata::open(&inv.home, true)
        .unwrap()
        .create("display/OP-history", &json!({}))
        .unwrap();
    let read = lycheedev_live::history::inspect_operation(&inv.home, "OP-history", &inv.control);
    assert!(read.error.is_some());
    let value = serde_json::to_value(&read.outcome).unwrap();
    assert_eq!(value["report"]["state"], "verified");
    assert_eq!(value["business"]["assertionsPassed"], true);
    assert_eq!(value["display"]["state"], "pending");
    assert_eq!(value["nextAction"]["args"][1], "abandon");
    let response = dispatch_history(&inv);
    assert_eq!(response.exit_code(), 5);
    assert!(envelope(&response)["result"].is_null());
}
#[test]
fn historical_session_real_entry_missing_has_no_warning_or_native_factory() {
    let f = Fixture::new();
    let inv = invocation(&["live", "session", "SESSION-missing"], &f.0);
    lycheedev_core::workspace::initialize(&inv.home).unwrap();
    let response = dispatch_history(&inv);
    assert_eq!(response.exit_code(), 2);
    assert!(response.warnings.is_empty());
    assert!(!response.context.contains_key("session"));
}

#[test]
fn historical_session_real_entry_authenticates_pin_manifest_blob_and_warning() {
    use lycheedev_core::{
        selection::{PinnedSet, SelectionSpec, SourcePin},
        storage,
    };
    let f = Fixture::new();
    let mut inv = invocation(&["live", "session", "SESSION-placeholder"], &f.0);
    lycheedev_core::workspace::initialize(&inv.home).unwrap();
    let mut pin = PinnedSet {
        schema: "lycheedev.selection.v1".into(),
        id: String::new(),
        spec: SelectionSpec {
            source: Some(SourcePin {
                repository: "wow-ui-source".into(),
                product: "retail".into(),
                requested_ref: "retail".into(),
                exact_commit: "a".repeat(40),
                parser_revision: "fixture".into(),
            }),
            ..SelectionSpec::default()
        },
    };
    pin.id = format!(
        "PIN-{}",
        storage::sha256(&storage::identity_json(&pin).unwrap())
    );
    Metadata::open(&inv.home, true)
        .unwrap()
        .create(&format!("pin/{}", pin.id), &pin)
        .unwrap();
    let mut target = ConnectionTarget::default();
    target.client.product = "retail".into();
    target.client.full_build = "12.1.0.12345".into();
    target.window.process_id = 73;
    target.window.handle = u64::MAX;
    target.window.process_started_at = 133_333_333_333_333_333;
    let nonce = "a".repeat(32);
    // Fixed Go struct order, not a serde_json::Value map. Session authenticates
    // exact canonical payload bytes before inspecting readiness.
    let ready = format!(
        r#"{{"schema":"lycheedev.signal.v1","release":"3.2.0","kind":"ready","sessionNonce":"{nonce}","requestId":"","character":"Character","realm":"Realm","guid":"Player-1-000001","product":"retail","build":"12.1.0.12345","sequence":1,"inputReady":true}}"#
    );
    let payload = format!(
        r#"{{"schema":"lycheedev.window-session.v1","target":{},"ready":{ready}}}"#,
        String::from_utf8(storage::identity_json(&target).unwrap()).unwrap()
    );
    let capture=evidence::commit_capture(&inv.home,payload.as_bytes(),"application/json",json!({"kind":"decoded-window-session","locator":format!("pid:73/window:{}",u64::MAX),"snapshot":pin.id,"dataBuild":"12.1.0.12345","session":nonce}),true,false).unwrap();
    let unsigned = format!(
        r#"{{"schema":"lycheedev.session.v1","id":"","snapshot":"{}","captureId":"{}","region":{{"Min":{{"X":0,"Y":0}},"Max":{{"X":0,"Y":0}}}},"bindings":{{"wakeBinding":"","submitBinding":"","closeBinding":""}}}}"#,
        pin.id,
        capture["id"].as_str().unwrap()
    );
    let id = format!("SESSION-{}", storage::sha256(unsigned.as_bytes()));
    let record = unsigned.replace("\"id\":\"\"", &format!("\"id\":\"{id}\""));
    let record = serde_json::value::RawValue::from_string(record).unwrap();
    Metadata::open(&inv.home, true)
        .unwrap()
        .create(&format!("session/{id}"), &record)
        .unwrap();
    inv.positional = Some(id.clone());
    let response = dispatch_history(&inv);
    assert_eq!(response.exit_code(), 0, "{:?}", response.error);
    let value = envelope(&response);
    assert_eq!(value["context"]["session"], id);
    assert_eq!(
        value["result"]["target"]["window"]["handle"],
        u64::MAX.to_string()
    );
    assert_eq!(
        value["result"]["target"]["window"]["processStartedAt"],
        "133333333333333333"
    );
    assert_eq!(
        value["warnings"],
        json!([
            "Historical session evidence only; live window identity and readiness have not been refreshed."
        ])
    );
    let blob = evidence::blob_path(&inv.home, capture["blob"]["sha256"].as_str().unwrap()).unwrap();
    fs::write(blob, b"changed").unwrap();
    let response = dispatch_history(&inv);
    assert_eq!(response.exit_code(), 4);
    assert_eq!(
        response.error.as_ref().unwrap().code,
        "vault.blob_integrity"
    );
    assert_eq!(
        response.error.as_ref().unwrap().message,
        "vault.blob_integrity"
    );
    assert!(response.warnings.is_empty());
    assert!(response.result.is_none());
}
