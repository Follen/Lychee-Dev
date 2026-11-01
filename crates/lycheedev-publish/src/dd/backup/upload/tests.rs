use super::*;
use crate::{
    dd::tests::transport,
    test_support::{Reply, Server, TestDir},
};
use std::fs;
fn reply(v: Value) -> Reply {
    Reply::Json(json!({"code":0,"result":v}))
}
fn clock() -> Reply {
    Reply::Json(json!({"code":200,"result":{"ts":1793500000}}))
}
fn fixture(temp: &TestDir, control: &OperationControl) -> (BackupUpload, Value) {
    let root = temp.0.join("game");
    fs::create_dir_all(root.join("Interface/AddOns/Selected")).unwrap();
    fs::write(
        root.join("Interface/AddOns/Selected/A.lua"),
        b"original addon bytes",
    )
    .unwrap();
    let output = temp.0.join("bundle");
    let input = json!({"schema":"lycheedev.publish.v1.dd.backup.generate","game_dir":root,"game_type":10001,"output_dir":output,"unknown_addons":["Selected"]});
    BackupGenerate::parse(&serde_json::to_vec(&input).unwrap(), &temp.0)
        .unwrap()
        .generate(control)
        .unwrap();
    let path = output.join("backup-bundle.json");
    let bundle: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    (
        BackupUpload::select(&path, None, control).unwrap(),
        bundle["manifest"].clone(),
    )
}
fn row() -> Value {
    json!({"sn":"new-sn","game_type":10001,"name":"backup","ctime":1793500000})
}
fn detail(manifest: &Value) -> Value {
    let mut v = manifest.clone();
    for (key, value) in row().as_object().unwrap() {
        v[key] = value.clone();
    }
    v
}
fn begin(url: Value) -> Vec<Reply> {
    vec![
        clock(),
        reply(json!([{"game_type":10001}])),
        reply(json!([])),
        reply(json!({"sn":"new-sn","unknown_addon":{"u_url":url}})),
    ]
}
#[test]
fn original_zip_uploaded_once_without_author_headers_and_completed_backup_is_reused() {
    let temp = TestDir::new();
    let control = OperationControl::default();
    let (upload, manifest) = fixture(&temp, &control);
    let mut replies = begin(json!(
        "http://a19-ui-res.s3v2.nie.netease.com/object?Signature=synthetic&content-type=application%2Fzip&x-amz-acl=private"
    ));
    replies.extend([
        Reply::Raw("HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
        reply(json!({})),
        reply(json!([row()])),
        reply(detail(&manifest)),
        reply(json!([row()])),
        reply(detail(&manifest)),
    ]);
    let server = Server::new(replies);
    let (mut api, _) = transport(&server, "synthetic-author");
    api.begin_operation(&control).unwrap();
    let home = temp.0.join("state");
    let result = send(&mut api, &home, "author", &upload.request, &control).unwrap();
    assert_eq!(result["uploaded_components"], json!(["unknown_addon"]));
    assert_eq!(result["verification_scope"], "cloud_selection_metadata");
    assert_eq!(
        send(&mut api, &home, "author", &upload.request, &control).unwrap()["recovered"],
        true
    );
    api.close().unwrap();
    let requests = server.finish();
    let put = requests
        .iter()
        .find(|r| r.line.starts_with("PUT "))
        .unwrap();
    assert_eq!(
        put.body,
        fs::read(temp.0.join("bundle/unknown_addon.zip")).unwrap()
    );
    let headers = put.headers.to_lowercase();
    assert!(headers.contains("content-type: application/zip"));
    assert!(headers.contains("x-amz-acl: private"));
    assert!(!headers.contains("cookie:"));
    assert!(!headers.contains("synthetic-author"));
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("POST /backup/pre_create"))
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("POST /backup/finish_create"))
            .count(),
        1
    );
    let pre = requests
        .iter()
        .find(|r| r.line.starts_with("POST /backup/pre_create"))
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&pre.body).unwrap(),
        manifest
    );
}
#[test]
fn server_reuse_and_lost_finish_response_recover_by_reads_without_upload_or_creation_replay() {
    let temp = TestDir::new();
    let control = OperationControl::default();
    let (upload, manifest) = fixture(&temp, &control);
    let mut replies = begin(Value::Null);
    replies.extend([Reply::Drop, reply(json!([row()])), reply(detail(&manifest))]);
    let server = Server::new(replies);
    let (mut api, _) = transport(&server, "synthetic-author");
    api.begin_operation(&control).unwrap();
    let home = temp.0.join("state");
    let error = send(&mut api, &home, "author", &upload.request, &control).unwrap_err();
    assert_eq!(error.exit_code, 6);
    assert_eq!(error.details["sn"], "new-sn");
    let recovered = send(&mut api, &home, "author", &upload.request, &control).unwrap();
    assert_eq!(recovered["reused_components"], json!(["unknown_addon"]));
    assert_eq!(recovered["recovered"], true);
    api.close().unwrap();
    let requests = server.finish();
    assert!(!requests.iter().any(|r| r.line.starts_with("PUT ")));
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("POST "))
            .count(),
        2
    );
}
#[test]
fn lost_put_keeps_the_original_pending_backup_and_does_not_replay_on_retry() {
    let temp = TestDir::new();
    let control = OperationControl::default();
    let (upload, _) = fixture(&temp, &control);
    let mut replies = begin(json!("https://upload.invalid/original?Signature=synthetic"));
    replies.push(Reply::Drop);
    let server = Server::new(replies);
    let (mut api, _) = transport(&server, "synthetic-author");
    api.begin_operation(&control).unwrap();
    let home = temp.0.join("state");
    assert_eq!(
        send(&mut api, &home, "author", &upload.request, &control)
            .unwrap_err()
            .exit_code,
        6
    );
    let error = send(&mut api, &home, "author", &upload.request, &control).unwrap_err();
    assert_eq!(error.details["stage"], "put_pending:unknown_addon");
    api.close().unwrap();
    let requests = server.finish();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("PUT "))
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("POST "))
            .count(),
        1
    );
}
