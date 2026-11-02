use super::super::tests::{client, me, reply};
use super::*;
use crate::test_support::{Reply, Server, TestDir};
use std::fs;
fn id(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}
fn request(dir: &TestDir) -> PluginVersion {
    PluginVersion::parse(br#"{"schema":"lycheedev.publish.v1.newbee.official.plugin-version.upload","id":91,"version":"1.2.3","game_versions":"auto","changelog":"PRIVATE_LOG","file":"AuthorAddon.zip"}"#,&dir.0,&OperationControl::default()).unwrap()
}
fn page(rows: Value, total: u64, page: u64) -> Reply {
    reply(json!({"list":rows,"total":total,"pagenum":page,"pagesize":2000}))
}
fn metadata() -> Reply {
    Reply::Json(json!({"game_version":[{"id":2,"version":["12.0.0","1.15.8"]}]}))
}
fn row(file: u64, size: u64) -> Value {
    json!({"file_id":file,"version_name":"1.2.3","file_size":size,"game_versions":["12.0.0"],"changelog":"PRIVATE_LOG"})
}
#[test]
fn lost_upload_uses_original_client_versions_and_requires_explicit_file_recovery() {
    let dir = TestDir::new();
    let path = dir.zip();
    let size = path.metadata().unwrap().len();
    let request = request(&dir);
    let server = Server::new(vec![
        me(71),
        reply(json!({"mod_id":91})),
        page(json!([]), 0, 1),
        metadata(),
        Reply::Drop,
        page(json!([row(500, size + 1)]), 1, 1),
        page(json!([row(500, size)]), 1, 1),
    ]);
    let mut c = client(&dir, &server);
    let error = c.upload_plugin_version(&request, None).unwrap_err();
    assert_eq!(error.exit_code, 5);
    assert_eq!(error.details["submission"], "uncertain");
    assert_eq!(
        c.upload_plugin_version(&request, None)
            .unwrap_err()
            .exit_code,
        6
    );
    assert_eq!(
        c.upload_plugin_version(&request, Some(id(500)))
            .unwrap_err()
            .exit_code,
        6
    );
    let result = c.upload_plugin_version(&request, Some(id(500))).unwrap();
    assert_eq!(result["file_id"], 500);
    assert_eq!(result["game_versions"], json!(["12.0.0"]));
    assert_eq!(result["remote_byte_verification"], "not_available");
    assert_eq!(result["changelog_verification"], "readback");
    assert!(!result.to_string().contains("PRIVATE_LOG"));
    let reused = c.upload_plugin_version(&request, None).unwrap();
    assert_eq!(reused["reused"], true);
    assert_eq!(reused["verification_source"], "original_receipt");
    let requests = server.finish();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("GET "))
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.contains("/upload_mod_file "))
            .count(),
        1
    );
    let body = &requests[4].body;
    for field in [
        "name=\"mod_id\"\r\n\r\n91",
        "name=\"game_version_list\"\r\n\r\n[\"12.0.0\"]",
        "PRIVATE_LOG",
    ] {
        assert!(body.windows(field.len()).any(|v| v == field.as_bytes()));
    }
    assert!(
        body.windows(fs::read(path).unwrap().len())
            .any(|v| v == fs::read(dir.0.join("AuthorAddon.zip")).unwrap())
    );
}
#[test]
fn acknowledged_upload_keeps_its_file_identity_and_original_version_before_retry() {
    let dir = TestDir::new();
    let size = dir.zip().metadata().unwrap().len();
    let request = request(&dir);
    let mut wrong = row(501, size);
    wrong["version_name"] = json!("other");
    let server = Server::new(vec![
        me(71),
        reply(json!({"mod_id":91})),
        page(json!([]), 0, 1),
        metadata(),
        reply(json!({"file_id":501,"token":"PRIVATE_TOKEN"})),
        page(json!([wrong]), 1, 1),
        page(json!([row(501, size)]), 1, 1),
    ]);
    let mut c = client(&dir, &server);
    assert_eq!(
        c.upload_plugin_version(&request, None)
            .unwrap_err()
            .exit_code,
        6
    );
    assert_eq!(
        c.upload_plugin_version(&request, Some(id(99)))
            .unwrap_err()
            .exit_code,
        4
    );
    let result = c.upload_plugin_version(&request, None).unwrap();
    assert_eq!(result["recovered"], true);
    assert_eq!(result["file_id"], 501);
    assert!(!result.to_string().contains("PRIVATE_TOKEN"));
    assert_eq!(
        server
            .finish()
            .iter()
            .filter(|r| r.line.contains("upload_mod_file "))
            .count(),
        1
    );
}
#[test]
fn version_collisions_scan_all_pages_and_incomplete_metadata_never_licenses_upload() {
    let dir = TestDir::new();
    dir.zip();
    let request = request(&dir);
    let server = Server::new(vec![
        me(71),
        reply(json!({"mod_id":91})),
        page(json!([{"file_id":700,"version_name":"newer"}]), 2, 1),
        page(json!([{"file_id":600,"version_name":"1.2.3"}]), 2, 2),
    ]);
    let mut c = client(&dir, &server);
    assert_eq!(
        c.upload_plugin_version(&request, None)
            .unwrap_err()
            .exit_code,
        4
    );
    let requests = server.finish();
    assert_eq!(input::value(&requests[3].body).unwrap()["pagenum"], 2);
    assert!(!requests.iter().any(|r| r.line.contains("upload_mod_file")));
    let server = Server::new(vec![
        me(71),
        reply(json!({"mod_id":91})),
        reply(json!({"list":[]})),
    ]);
    assert_eq!(
        client(&dir, &server)
            .upload_plugin_version(&request, None)
            .unwrap_err()
            .exit_code,
        5
    );
    server.finish();
}
#[test]
fn project_upload_defaults_package_once_and_reuses_the_same_archive_and_receipt() {
    let dir = TestDir::new();
    let root = dir.0.join("Project");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("Project.toc"), b"## Interface: 120000\n").unwrap();
    fs::write(root.join("main.lua"), b"print('native')").unwrap();
    fs::write(root.join("CHANGELOG.md"), "PRIVATE_LOG").unwrap();
    fs::write(root.join("ncc.json"),br#"{"id":91,"version":"1.2.3","game_versions":["12.0.0"],"changelog_file":"CHANGELOG.md","ignore":["CHANGELOG.md"]}"#).unwrap();
    let request=PluginVersion::parse(br#"{"schema":"lycheedev.publish.v1.newbee.official.plugin-version.upload","dir":"Project"}"#,&dir.0,&OperationControl::default()).unwrap();
    let plan = request.plan(&OperationControl::default()).unwrap();
    assert_eq!(plan["archive_validation"], "not_built");
    assert!(!dir.0.join("Project-1.2.3.zip").exists());
    let pack =
        PluginPack::for_upload(&root, &dir.0, "1.2.3", &OperationControl::default()).unwrap();
    let package = pack.run(&OperationControl::default()).unwrap();
    let size = package["zip_bytes"].as_u64().unwrap();
    let before = fs::read(pack.output()).unwrap();
    let server = Server::new(vec![
        me(71),
        reply(json!({"mod_id":91})),
        page(json!([]), 0, 1),
        metadata(),
        reply(json!({"file_id":502})),
        page(json!([row(502, size)]), 1, 1),
    ]);
    let mut c = client(&dir, &server);
    let result = c.upload_plugin_version(&request, None).unwrap();
    assert_eq!(result["package"]["reused"], true);
    assert_eq!(
        c.upload_plugin_version(&request, None).unwrap()["reused"],
        true
    );
    assert_eq!(fs::read(pack.output()).unwrap(), before);
    assert_eq!(server.finish().len(), 6);
}
#[test]
fn changed_pending_log_is_rejected_and_unknown_client_versions_fail_before_post() {
    let dir = TestDir::new();
    dir.zip();
    let request = request(&dir);
    let server = Server::new(vec![
        me(71),
        reply(json!({"mod_id":91})),
        page(json!([]), 0, 1),
        metadata(),
        Reply::Drop,
    ]);
    let mut c = client(&dir, &server);
    assert_eq!(
        c.upload_plugin_version(&request, None)
            .unwrap_err()
            .exit_code,
        5
    );
    let changed=PluginVersion::parse(br#"{"schema":"lycheedev.publish.v1.newbee.official.plugin-version.upload","id":91,"version":"1.2.3","game_versions":"auto","changelog":"different","file":"AuthorAddon.zip"}"#,&dir.0,&OperationControl::default()).unwrap();
    assert_eq!(
        c.upload_plugin_version(&changed, None)
            .unwrap_err()
            .exit_code,
        6
    );
    server.finish();
    let dir = TestDir::new();
    dir.zip();
    let bad=PluginVersion::parse(br#"{"schema":"lycheedev.publish.v1.newbee.official.plugin-version.upload","id":91,"version":"1.2.3","game_versions":["unknown"],"changelog":"","file":"AuthorAddon.zip"}"#,&dir.0,&OperationControl::default()).unwrap();
    let server = Server::new(vec![
        me(71),
        reply(json!({"mod_id":91})),
        page(json!([]), 0, 1),
        metadata(),
    ]);
    assert_eq!(
        client(&dir, &server)
            .upload_plugin_version(&bad, None)
            .unwrap_err()
            .exit_code,
        4
    );
    server.finish();
}
