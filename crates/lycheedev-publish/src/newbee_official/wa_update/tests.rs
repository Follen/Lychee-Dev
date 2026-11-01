use super::*;
use crate::{
    newbee_official::tests::{client, me, reply},
    test_support::{Reply, Server, TestDir},
};
use std::fs;
const OPAQUE: &str = "!WA:2!PRIVATE_OFFICIAL_UPDATED_CONTENT";
fn request(dir: &TestDir, version: Option<&str>) -> WaFileUpdate {
    fs::write(dir.0.join("wa.txt"), OPAQUE).unwrap();
    let mut doc = json!({"schema":"lycheedev.publish.v1.newbee.official.wa.update","id":91,"file":"wa.txt","wa_log":"Changes"});
    if let Some(version) = version {
        doc["version"] = json!(version);
    }
    WaFileUpdate::parse(&serde_json::to_vec(&doc).unwrap(), &dir.0).unwrap()
}
fn state(version: &str, content: &str) -> Reply {
    reply(json!({"wa_id":91,"version":version,"wa_str":content,"wa_log":"Changes"}))
}
fn single() -> Reply {
    reply(json!({"id":91,"is_multi_string":false}))
}
#[test]
fn official_file_update_selects_and_persists_the_patch_then_verifies_the_original_version_and_bytes()
 {
    let dir = TestDir::new();
    let request = request(&dir, None);
    let plan = request.plan(&OperationControl::default()).unwrap();
    assert_eq!(plan["credential_reads"], false);
    assert!(!plan.to_string().contains(OPAQUE));
    assert!(!dir.0.join("publish").exists());
    let server = Server::new(vec![
        me(71),
        state("1.2.3", "old"),
        single(),
        reply(json!({"version":"1.2.4"})),
        state("1.2.4", OPAQUE),
        reply(json!({"wa_id":91,"version":"1.2.4","wa_str":OPAQUE,"wa_log":null})),
    ]);
    let mut client = client(&dir, &server);
    let result = client.update_wa_file(&request).unwrap();
    assert_eq!(result["version"], "1.2.4");
    assert_eq!(result["log_verification"], "readback");
    assert!(!result.to_string().contains(OPAQUE));
    let reused = client.update_wa_file(&request).unwrap();
    assert_eq!(reused["reused"], true);
    assert_eq!(reused["original_log_verification"], "readback");
    assert_eq!(reused["log_verification"], "not_available");
    let requests = server.finish();
    assert_eq!(requests.len(), 6);
    assert_eq!(
        requests[3].line,
        "POST /creator/cli/v1/wa/push_str HTTP/1.1"
    );
    assert_eq!(
        input::value(&requests[3].body).unwrap(),
        json!({"id":91,"wa_str":OPAQUE,"wa_log":"Changes","version":"1.2.4"})
    );
}
#[test]
fn lost_official_update_does_not_reselect_or_replay_and_rejects_changed_input_wrong_version_and_identity()
 {
    let dir = TestDir::new();
    let request = request(&dir, None);
    let server = Server::new(vec![
        me(71),
        state("1.0.0", "old"),
        single(),
        Reply::Drop,
        state("1.0.2", OPAQUE),
        reply(json!({"wa_id":92,"version":"1.0.1","wa_str":OPAQUE})),
        state("1.0.1", OPAQUE),
    ]);
    let mut client = client(&dir, &server);
    assert_eq!(
        client.update_wa_file(&request).unwrap_err().details["submission"],
        "uncertain"
    );
    fs::write(dir.0.join("wa.txt"), "changed while pending").unwrap();
    assert_eq!(client.update_wa_file(&request).unwrap_err().exit_code, 6);
    fs::write(dir.0.join("wa.txt"), OPAQUE).unwrap();
    assert_eq!(client.update_wa_file(&request).unwrap_err().exit_code, 6);
    assert_eq!(client.update_wa_file(&request).unwrap_err().exit_code, 6);
    let recovered = client.update_wa_file(&request).unwrap();
    assert_eq!(recovered["version"], "1.0.1");
    assert_eq!(recovered["recovered"], true);
    assert_eq!(recovered["author_writes"], 0);
    let requests = server.finish();
    assert_eq!(requests.len(), 7);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.line == "POST /creator/cli/v1/wa/push_str HTTP/1.1")
            .count(),
        1
    );
}
#[test]
fn official_string_collections_are_rejected_before_submitting_a_single_string_update() {
    let dir = TestDir::new();
    let request = request(&dir, None);
    let server = Server::new(vec![
        me(71),
        state("1.0.0", "collection"),
        reply(json!({"id":91,"is_multi_string":true})),
    ]);
    let mut client = client(&dir, &server);
    assert_eq!(client.update_wa_file(&request).unwrap_err().exit_code, 3);
    assert_eq!(server.finish().len(), 3);
}
#[test]
fn legacy_versions_require_an_explicit_target_before_the_official_request() {
    let dir = TestDir::new();
    let automatic = request(&dir, None);
    let explicit = request(&dir, Some("2.0.0"));
    let server = Server::new(vec![
        me(71),
        state("1", "old"),
        single(),
        state("1", "old"),
        single(),
        reply(Value::Null),
        state("2.0.0", OPAQUE),
    ]);
    let mut client = client(&dir, &server);
    assert_eq!(client.update_wa_file(&automatic).unwrap_err().exit_code, 4);
    assert_eq!(
        client.update_wa_file(&explicit).unwrap()["version"],
        "2.0.0"
    );
    let requests = server.finish();
    assert_eq!(requests.len(), 7);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.line.ends_with("/wa/push_str HTTP/1.1"))
            .count(),
        1
    );
}
#[test]
fn invalid_files_and_versions_fail_before_authentication_and_missing_remote_logs_remain_unverified()
{
    let dir = TestDir::new();
    let request = request(&dir, Some("1.2.3"));
    fs::write(dir.0.join("wa.txt"), [0xff]).unwrap();
    assert_eq!(
        request
            .plan(&OperationControl::default())
            .unwrap_err()
            .exit_code,
        4
    );
    fs::write(dir.0.join("wa.txt"), "  \n").unwrap();
    assert_eq!(
        request
            .plan(&OperationControl::default())
            .unwrap_err()
            .exit_code,
        4
    );
    for target in [json!("1.2"), json!("01.2.3"), Value::Null] {
        let doc = json!({"schema":"lycheedev.publish.v1.newbee.official.wa.update","id":91,"file":"wa.txt","wa_log":"","version":target});
        assert_eq!(
            WaFileUpdate::parse(&serde_json::to_vec(&doc).unwrap(), &dir.0)
                .err()
                .unwrap()
                .exit_code,
            4
        );
    }
    assert!(
        current(
            json!({"version":"1.0.0","wa_str":"opaque","wa_log":null}),
            NonZeroU64::new(91).unwrap()
        )
        .unwrap()
        .log_sha256
        .is_none()
    );
}
