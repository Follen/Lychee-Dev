use super::*;
use crate::test_support::{Reply, Server, TestDir};
use std::fs;
pub(super) fn reply(value: Value) -> Reply {
    Reply::Json(json!({"code":1,"data":value}))
}
pub(super) fn me(id: u64) -> Reply {
    reply(
        json!({"uid":id,"username":"Fixture Author","modules":["wow"],"token_name":"PRIVATE_TOKEN_NAME","token_prefix":"PRIVATE_PREFIX","token_created_at":"PRIVATE_DATE"}),
    )
}
pub(super) fn client(dir: &TestDir, server: &Server) -> Official {
    let control = OperationControl::default();
    Official {
        home: dir.0.clone(),
        http: Http::loopback(&control, server.url.clone()),
        control,
        session: Some(Session::token("ncc_SYNTHETIC_CREDENTIAL", "environment").unwrap()),
        author: None,
    }
}
fn id(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}
#[test]
fn official_profile_credentials_are_private_bounded_and_cannot_override_the_fixed_origin() {
    let dir = TestDir::new();
    fs::create_dir(dir.0.join(".ncc")).unwrap();
    let path = dir.0.join(".ncc/config.json");
    fs::write(
        &path,
        br#"{"baseUrl":"http://untrusted.invalid","token":"ncc_SYNTHETIC_CREDENTIAL"}"#,
    )
    .unwrap();
    let session = Session::profile(&dir.0, &OperationControl::default()).unwrap();
    assert_eq!(session.source, "official_config");
    assert!(session.headers["authorization"].is_sensitive());
    assert_eq!(session.headers["x-ncc-cli-version"], "0.1.29");
    fs::write(
        path,
        br#"{"token":"ncc_SECRET","token":"ncc_OTHER_SECRET"}"#,
    )
    .unwrap();
    let error = Session::profile(&dir.0, &OperationControl::default())
        .err()
        .unwrap();
    assert_eq!(error.exit_code, 3);
    assert!(!error.to_string().contains("SECRET"));
    assert!(Session::token("ncc_bad\r\nheader", "environment").is_err());
}
#[test]
fn official_query_routes_keep_resource_namespaces_filters_exact_ids_and_selected_page_coverage() {
    let dir = TestDir::new();
    let server = Server::new(vec![
        me(71),
        reply(json!({"list":[],"total":0})),
        reply(json!({"list":[],"total":0})),
        reply(json!({"list":[],"total":0})),
        reply(json!({"list":[]})),
        reply(json!({"list":[]})),
        reply(json!({"id":9007199254740993u64,"title":"UI"})),
        reply(json!({"items":[]})),
    ]);
    let mut client = client(&dir, &server);
    let doctor = client.query(&Query::Doctor).unwrap();
    assert_eq!(doctor["uid"], 71);
    assert!(!doctor.to_string().contains("PRIVATE_"));
    for (resource, paid, size) in [
        (Resource::Plugin, None, Some(20)),
        (Resource::Wa, Some(false), None),
        (Resource::Config, Some(true), None),
    ] {
        let value = client
            .query(&Query::List(
                AuthorList::new(resource, id(2), size, "keyword".into(), paid).unwrap(),
            ))
            .unwrap();
        assert_eq!(value["coverage"], "unknown");
        assert_eq!(value["page"], 2);
    }
    client
        .query(&Query::PluginVersions {
            id: id(91),
            page: id(3),
            size: 100,
        })
        .unwrap();
    client
        .query(&Query::Comments {
            resource: Resource::Wa,
            id: id(91),
            page: id(4),
            status: ReplyStatus::Unreplied,
        })
        .unwrap();
    let reference = client
        .query(&Query::Reference {
            resource: Resource::Config,
            id: id(9007199254740993),
            link: false,
            game_version: None,
        })
        .unwrap();
    assert_eq!(
        reference["reference_code"],
        "{[shareConfig=-=9007199254740993=-=UI=-=1]}"
    );
    client.query(&Query::Backups).unwrap();
    let requests = server.finish();
    let expected = [
        ("/me", json!({})),
        (
            "/mod/publish_list",
            json!({"keyword":"keyword","pagenum":2,"pagesize":20}),
        ),
        (
            "/wa/publish_list",
            json!({"keyword":"keyword","pagenum":2,"is_paid":false}),
        ),
        (
            "/share_config/publish_list",
            json!({"keyword":"keyword","pagenum":2,"is_paid":true}),
        ),
        (
            "/mod_file/mod_file_list",
            json!({"mod_id":91,"pagenum":3,"pagesize":100}),
        ),
        (
            "/content_review/list",
            json!({"content_type":2,"content_id":91,"reply_status":1,"page":4}),
        ),
        (
            "/share_config/publish_detail",
            json!({"id":9007199254740993u64}),
        ),
        ("/cloudbackup/list", json!({})),
    ];
    for (request, (path, body)) in requests.iter().zip(expected) {
        assert_eq!(request.line, format!("POST /creator/cli/v1{path} HTTP/1.1"));
        assert_eq!(input::value(&request.body).unwrap(), body);
        assert!(
            request
                .headers
                .contains("authorization: Bearer ncc_SYNTHETIC_CREDENTIAL")
        );
        assert!(request.headers.contains("x-ncc-cli-version: 0.1.29"));
        assert!(!request.headers.contains("boxversion:"));
    }
    assert!(AuthorList::new(Resource::Plugin, id(1), Some(9), String::new(), None).is_err());
    assert!(AuthorList::new(Resource::Wa, id(1), Some(20), String::new(), None).is_err());
}
fn media(dir: &TestDir) -> MediaUpload {
    fs::write(dir.0.join("image.png"), b"fixture-media-bytes").unwrap();
    MediaUpload::parse(br#"{"schema":"lycheedev.publish.v1.newbee.official.media.upload","resource":"plugin","file":"image.png"}"#,&dir.0).unwrap()
}
#[test]
fn native_official_media_streams_locked_bytes_and_reuses_uploads_within_the_author_identity() {
    let dir = TestDir::new();
    let request = media(&dir);
    let server = Server::new(vec![
        me(71),
        reply(json!({"media_url":"https://cdn.invalid/a.png"})),
        me(72),
        reply(json!({"media_url":"https://cdn.invalid/b.png"})),
    ]);
    let plan = request.plan(&OperationControl::default()).unwrap();
    assert_eq!(plan["credential_reads"], false);
    assert!(!dir.0.join("publish").exists());
    let mut first = client(&dir, &server);
    let result = first.upload_media(&request).unwrap();
    assert_eq!(result["media_url"], "https://cdn.invalid/a.png");
    assert_eq!(first.upload_media(&request).unwrap()["reused"], true);
    let mut second = client(&dir, &server);
    assert_eq!(
        second.upload_media(&request).unwrap()["media_url"],
        "https://cdn.invalid/b.png"
    );
    let requests = server.finish();
    assert_eq!(requests.len(), 4);
    for index in [1, 3] {
        let request = &requests[index];
        assert_eq!(
            request.line,
            "POST /creator/cli/v1/mod/upload_media HTTP/1.1"
        );
        let body = String::from_utf8_lossy(&request.body);
        assert!(body.contains("fixture-media-bytes"));
        assert!(body.contains("Content-Type: image/png"));
    }
}
#[test]
fn official_categories_and_reference_versions_are_selected_from_fresh_public_metadata() {
    let dir = TestDir::new();
    let metadata = || Reply::Json(json!({"game_version":[{"id":2,"name":"Fixture"}]}));
    let server = Server::new(vec![
        metadata(),
        metadata(),
        reply(json!([{"id":17,"name":"Category"}])),
        metadata(),
        reply(json!({"id":91,"name":"Addon"})),
    ]);
    let mut client = client(&dir, &server);
    assert_eq!(
        client
            .query(&Query::WaCategories {
                game_version: id(999)
            })
            .unwrap_err()
            .exit_code,
        4
    );
    let categories = client
        .query(&Query::WaCategories {
            game_version: id(2),
        })
        .unwrap();
    assert_eq!(categories[0]["id"], 17);
    let reference = client
        .query(&Query::Reference {
            resource: Resource::Plugin,
            id: id(91),
            link: true,
            game_version: Some(id(2)),
        })
        .unwrap();
    assert_eq!(
        reference["reference_code"],
        "{[plugin=-=91=-=Addon=-=2=-=2]}"
    );
    let requests = server.finish();
    assert_eq!(requests.len(), 5);
    for index in [0, 1, 3] {
        assert_eq!(requests[index].line, "GET /modconfig.json HTTP/1.1");
        assert!(!requests[index].headers.contains("authorization:"));
    }
    assert_eq!(
        input::value(&requests[2].body).unwrap(),
        json!({"game_version":2})
    );
}
#[test]
fn uncertain_official_media_is_retained_and_business_errors_never_echo_tokens_or_vendor_upgrade_commands()
 {
    let dir = TestDir::new();
    let request = media(&dir);
    let server = Server::new(vec![me(71), Reply::Drop]);
    let mut client = client(&dir, &server);
    assert_eq!(
        client.upload_media(&request).unwrap_err().details["submission"],
        "uncertain"
    );
    assert_eq!(client.upload_media(&request).unwrap_err().exit_code, 6);
    assert_eq!(server.finish().len(), 2);
    for (code, kind, exit) in [
        (-3, "auth_failed", 3),
        (0, "cli_version_outdated", 3),
        (0, "quota_exceeded", 5),
        (0, "cli_disabled", 3),
    ] {
        let error=envelope(json!({"code":code,"message":"PRIVATE_TOKEN ncc_SECRET npm install -g vendor@latest","data":{"error":kind,"token":"PRIVATE_TOKEN"}}),true).unwrap_err();
        assert_eq!(error.exit_code, exit);
        assert_eq!(error.details["submission"], "rejected");
        let text = serde_json::to_string(&error).unwrap();
        assert!(!text.contains("PRIVATE_"));
        assert!(!text.contains("@latest"));
    }
    assert_eq!(
        envelope(json!({"message":"PRIVATE_TOKEN"}), true)
            .unwrap_err()
            .details["submission"],
        "uncertain"
    );
}
