use super::*;
use crate::{
    newbee::tests::provider,
    test_support::{Reply, Server, TestDir},
};
const CODE: &str = "900150983cd24fb0d6963f7d28e17f72";
#[test]
fn authenticated_image_receipts_are_account_scoped_and_survive_switching_back() {
    let dir = TestDir::new();
    let path = dir.0.join("image.png");
    std::fs::write(&path, b"same-image").unwrap();
    let mut replies = Vec::new();
    for value in [
        "https://cdn.invalid/author-a.png",
        "https://cdn.invalid/author-b.png",
    ] {
        replies.extend(crate::newbee::tests::authenticated(vec![Reply::Json(
            json!({"code":1,"data":{"media_url":value}}),
        )]));
    }
    let server = Server::new(replies);
    for (subject, url) in [
        ("author-A", "https://cdn.invalid/author-a.png"),
        ("author-B", "https://cdn.invalid/author-b.png"),
        ("author-A", "https://cdn.invalid/author-a.png"),
    ] {
        let mut client = provider(&dir, &server, true);
        crate::newbee::tests::set_subject(&dir, subject);
        let result = client.upload_image(Resource::Plugin, &path).unwrap();
        assert_eq!(result["url"], url);
    }
    let requests = server.finish();
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.line == "POST /creator/wow/mod/upload_media HTTP/1.1")
            .count(),
        2
    );
    assert_eq!(requests.len(), 8);
}
fn prepare() -> Value {
    let mut result = json!({"code":1,"data":{"id":"original-index","items":{}}});
    result["data"]["items"][CODE] = json!({"exists":false,"url":"https://objects.invalid/opaque?signature=private-capability","callback":"private-callback"});
    result
}
fn index() -> Value {
    json!({"code":1,"data":{"code":"reusable-content-index","fileName":"notes.txt","totalSize":3}})
}
#[test]
fn attachment_streams_the_locked_object_without_creator_auth_or_an_extra_content_type() {
    let dir = TestDir::new();
    let path = dir.0.join("notes.txt");
    std::fs::write(&path, b"abc").unwrap();
    let server = Server::new(vec![
        Reply::Json(prepare()),
        Reply::Raw("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK"),
        Reply::Json(index()),
    ]);
    let mut provider = provider(&dir, &server, false);
    let result = provider
        .upload_attachment(&path, Some(2), Some("Interface"))
        .unwrap();
    assert_eq!(
        result["attachment"],
        json!({"name":"notes.txt","value":"reusable-content-index","is_compressed":true,"timestamp":0,"install_type":2,"install_path":"Interface"})
    );
    assert_eq!(
        provider
            .upload_attachment(&path, Some(2), Some("Interface"))
            .unwrap(),
        result
    );
    assert!(provider.session.is_none());
    let requests = server.finish();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1].body, b"abc");
    assert_eq!(
        requests[1].line,
        "PUT /opaque?signature=private-capability HTTP/1.1"
    );
    assert!(
        requests[1]
            .headers
            .contains("x-oss-callback: private-callback")
    );
    assert!(
        !requests[1]
            .headers
            .to_ascii_lowercase()
            .contains("content-type:")
    );
    assert!(requests.iter().all(|request| {
        !request
            .headers
            .to_ascii_lowercase()
            .contains("authorization:")
    }));
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["code"], CODE);
    assert_eq!(body["files"][0]["chunks"][0], json!({"hash":CODE,"size":3}));
    assert!(!result.to_string().contains("private-"));
}
#[test]
fn lost_object_response_recovers_by_original_content_index_without_a_second_put() {
    let dir = TestDir::new();
    let path = dir.0.join("notes.txt");
    std::fs::write(&path, b"abc").unwrap();
    let server = Server::new(vec![
        Reply::Json(prepare()),
        Reply::Drop,
        Reply::Json(index()),
    ]);
    let mut provider = provider(&dir, &server, false);
    let error = provider.upload_attachment(&path, None, None).unwrap_err();
    assert_eq!(error.details["submission"], "uncertain");
    let wire = serde_json::to_string(&error).unwrap();
    assert!(!wire.contains("private-capability"));
    assert!(!wire.contains("private-callback"));
    let result = provider.upload_attachment(&path, None, None).unwrap();
    assert_eq!(result["upload"]["value"], "reusable-content-index");
    let requests = server.finish();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.line.starts_with("PUT "))
            .count(),
        1
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[2].body).unwrap(),
        json!({"code":"original-index"})
    );
}
#[test]
fn signed_object_urls_keep_the_known_backup_tls_upgrade_and_reject_other_cleartext() {
    assert_eq!(
        object_url(
            "http://wclboxdata.cn-hangzhou.oss.aliyuncs.com/a?Signature=secret",
            true
        )
        .unwrap()
        .as_str(),
        "https://wclboxdata.cn-hangzhou.oss.aliyuncs.com/a?Signature=secret"
    );
    for value in [
        "http://objects.invalid/a",
        "https://user@objects.invalid/a",
        "https://objects.invalid/a#secret",
        "file:///private",
    ] {
        assert!(object_url(value, false).is_err());
    }
    assert!(object_url("http://wclboxdata.cn-hangzhou.oss.aliyuncs.com/a", false).is_err());
}
