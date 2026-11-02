use super::{
    protocol::{Api, Session, sign},
    *,
};
use crate::{
    test_support::{Reply, Server},
    transport::Http,
};
use std::time::Duration;
pub(super) fn api(server: &Server) -> (Api, Session) {
    let control =
        OperationControl::uncancelable(std::time::Instant::now() + Duration::from_secs(20));
    let api = Api {
        http: Http::loopback(&control, server.url.clone()),
    };
    let mut session = Session {
        method: "sms".into(),
        device_id: "fixture-device".into(),
        ..Default::default()
    };
    session.apply(&json!({"profile":{"heybox_id":"123"},"pkey":"fixture-private-key","expire_at":now()+3600}),None).unwrap();
    (api, session)
}
pub(super) fn ok(result: Value) -> Reply {
    Reply::Json(json!({"status":"ok","result":result}))
}
#[test]
fn independent_signer_vectors_and_native_forms_match_workshop_protocol() {
    assert_eq!(
        sign("/account/restore_login", 1700000000, &"A".repeat(32)),
        "T3IS037"
    );
    assert_eq!(
        sign("/wow/v2/share/list/", 1700000000, &"A".repeat(32)),
        "3VXIS16"
    );
    let server = Server::new(vec![ok(
        json!({"strappidInfos":[{"strappid":"wowretail","name":"正式服"}]}),
    )]);
    let (api, mut s) = api(&server);
    let result = read::execute(&api, &mut s, Query::GameTypes).unwrap();
    assert_eq!(result["items"][0]["strappid"], "wowretail");
    let requests = server.finish();
    assert!(requests[0].headers.contains("heybox_id=123"));
    assert!(requests[0].line.contains("heybox_id=123"));
    assert!(requests[0].line.contains("hkey="));
    let foreign = reqwest::Url::parse("https://unrelated.invalid/connect/qrconnect").unwrap();
    assert!(protocol::oauth_origin(&foreign).is_err());
}
#[test]
fn version_and_config_lists_continue_short_pages_and_keep_partial_on_duplicate() {
    let server = Server::new(vec![
        ok(json!({"versionList":[{"id":1}],"totalCount":2})),
        ok(json!({"versionList":[{"id":2}],"totalCount":2})),
        ok(json!({"shares":[{"ShareID":"opaque-1"}]})),
        ok(json!({"shares":[{"ShareID":"opaque-2"}]})),
        ok(json!({"shares":[]})),
        ok(json!({"versionList":[{"id":1}]})),
        ok(json!({"versionList":[{"id":1}]})),
    ]);
    let (api, mut s) = api(&server);
    let versions = read::execute(&api, &mut s, Query::PluginVersions(719)).unwrap();
    assert_eq!(versions["items"].as_array().unwrap().len(), 2);
    let shares = read::execute(&api, &mut s, Query::Configs("wowretail".into())).unwrap();
    assert_eq!(shares["complete"], true);
    assert_eq!(shares["total_count"], 2);
    let error = read::execute(&api, &mut s, Query::PluginVersions(719)).unwrap_err();
    assert_eq!(error.details["coverage"], "partial");
    assert_eq!(error.details["absence_proven"], false);
    assert_eq!(error.details["items"].as_array().unwrap().len(), 1);
    let requests = server.finish();
    assert!(requests[1].line.contains("offset=1"));
    assert!(requests[3].line.contains("offset=1"));
    assert!(requests[4].line.contains("offset=2"));
}
#[test]
fn wa_reads_bind_author_latest_version_and_summarize_original_imports() {
    let server = Server::new(vec![
        ok(json!({"heybox_id":"123"})),
        ok(
            json!({"waList":[{"Slug":"opaque","HeyboxId":"123","LatestVersion":{"Version":9007199254740993u64}}]}),
        ),
        ok(json!({"waList":[]})),
        ok(
            json!({"Slug":"opaque","HeyboxId":"123","encoded":"!WA:2!private-original","name":"Visible title"}),
        ),
        ok(json!({"heybox_id":"999"})),
        ok(json!({"ShareID":"other","BackupID":"backup"})),
    ]);
    let (api, mut s) = api(&server);
    let result = read::execute(
        &api,
        &mut s,
        Query::Wa {
            branch: "wowretail".into(),
            slug: "opaque".into(),
        },
    )
    .unwrap();
    let public = crate::output::public(result, &[]);
    assert_eq!(public["wa"]["name"], "Visible title");
    assert!(public["wa"]["encoded"]["sha256"].is_string());
    assert!(!public.to_string().contains("private-original"));
    assert!(read::execute(&api, &mut s, Query::Was("wowretail".into())).is_err());
    assert!(read::execute(&api, &mut s, Query::Config("original".into())).is_err());
    let requests = server.finish();
    assert!(requests[3].line.contains("version_id=9007199254740993"));
}
