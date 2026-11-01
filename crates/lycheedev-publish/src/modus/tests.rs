use super::*;
use crate::{
    test_support::{Reply, Server},
    transport::Http,
};
pub(super) fn api(server: &Server, client: Client) -> protocol::Api {
    protocol::Api {
        http: Http::loopback(&leveldb::tests::control(), server.url.clone()),
        session: Some(auth::Session {
            client,
            token: "synthetic-private-token".into(),
            device: if client == Client::Main {
                Some("synthetic-device".into())
            } else {
                None
            },
            build: Some(3),
        }),
    }
}
fn ok(value: Value) -> Reply {
    Reply::Json(json!({"code":200,"data":value}))
}
#[test]
fn native_creator_pagination_preserves_exact_ids_filters_and_explicit_page_coverage() {
    let server = Server::new(vec![
        ok(json!({"total":2,"rows":[{"id":9007199254740993u64,"name":"One"}]})),
        ok(json!({"total":2,"rows":[{"id":9007199254740994u64,"name":"Two"}]})),
        ok(json!({"total":70,"rows":[{"fileId":77,"projectId":200000719}]})),
        ok(json!({"fileId":77,"projectId":200000999})),
    ]);
    let api = api(&server, Client::Creator);
    let result = read::execute(
        &api,
        Query::List {
            kind: Listing::Projects,
            build: None,
            page: None,
            size: 50,
            filters: json!({"keyword":"作者"}),
        },
    )
    .unwrap();
    assert_eq!(result["complete"], true);
    assert_eq!(result["items"][1]["id"].as_u64(), Some(9007199254740994));
    let result = read::execute(
        &api,
        Query::List {
            kind: Listing::Releases(200000719),
            build: None,
            page: Some(3),
            size: 7,
            filters: json!({}),
        },
    )
    .unwrap();
    assert_eq!(result["coverage"], "page");
    assert_eq!(result["complete"], false);
    assert!(
        read::execute(
            &api,
            Query::Release {
                project: 200000719,
                file: 77
            }
        )
        .is_err()
    );
    let requests = server.finish();
    assert!(
        requests
            .iter()
            .all(|r| r.headers.contains("Bearer synthetic-private-token")
                && !r.headers.contains("x-server-type"))
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[1].body).unwrap(),
        json!({"pageNum":2,"pageSize":50,"keyword":"作者"})
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[2].body).unwrap(),
        json!({"pageNum":3,"pageSize":7,"projectId":200000719})
    );
    assert!(
        requests[3]
            .line
            .starts_with("GET /api/game/data/author/project/file/detail/77 ")
    );
}
#[test]
fn main_filters_season_identity_and_public_choices_keep_correct_auth_and_payloads() {
    let server = Server::new(vec![
        ok(
            json!({"total":1,"rows":[{"id":"9007199254740993","codeText":"!WA:2!private","content":"private-html","title":"Visible"}]}),
        ),
        ok(json!({"user":{"userId":99}})),
        Reply::Json(json!({"code":200})),
        Reply::Json(json!({"rows":[{"name":"WeakAuras"}]})),
        ok(json!([{"id":"requested","content":"private-config"}])),
    ]);
    let api = api(&server, Client::Main);
    let result = read::execute(
        &api,
        Query::List {
            kind: Listing::Was,
            build: None,
            page: None,
            size: 20,
            filters: json!({"mine":true,"isPaid":0,"tags":["收藏","团队"],"status":0}),
        },
    )
    .unwrap();
    let output = crate::output::public(result, &[]).to_string();
    assert!(output.contains("Visible"));
    assert!(!output.contains("private-html"));
    assert!(!output.contains("!WA:"));
    assert_eq!(
        read::execute(
            &api,
            Query::Options {
                action: "season-subscription".into(),
                keys: vec![],
                author: None,
                game_type: Some("wow_anniversary".into())
            }
        )
        .unwrap()["data"],
        Value::Null
    );
    assert_eq!(
        read::execute(
            &api,
            Query::Options {
                action: "wa-support-addons".into(),
                keys: vec![],
                author: None,
                game_type: None
            }
        )
        .unwrap()["data"][0]["name"],
        "WeakAuras"
    );
    read::execute(
        &api,
        Query::Config {
            id: "requested".into(),
            build: Some(0),
        },
    )
    .unwrap();
    let req = server.finish();
    assert!(
        req[0]
            .headers
            .contains("authorization: synthetic-private-token")
    );
    assert!(req[0].headers.contains("x-server-type: 3"));
    assert_eq!(
        serde_json::from_slice::<Value>(&req[0].body).unwrap(),
        json!({"mine":true,"isPaid":0,"tags":["收藏","团队"],"status":0,"platform":0,"server":3,"pageNum":1,"pageSize":20})
    );
    assert!(!req[2].headers.contains("x-server-type"));
    assert!(req[2].line.contains("authorId=99&gameType=wow_anniversary"));
    assert!(!req[3].headers.contains("authorization"));
    assert!(!req[3].headers.contains("synthetic-device"));
    assert_eq!(req[4].line, "POST /api/system/user/share/detail HTTP/1.1");
    assert!(req[4].headers.contains("x-server-type: 0"));
}
#[test]
fn incomplete_pages_cannot_prove_absence_and_platform_errors_do_not_echo_secrets() {
    for second in [
        ok(json!({"total":2,"rows":[{"id":1}]})),
        Reply::Drop,
        Reply::Json(json!({"success":false,"code":403,"message":"synthetic-private-token"})),
    ] {
        let server = Server::new(vec![ok(json!({"total":2,"rows":[{"id":1}]})), second]);
        let api = api(&server, Client::Creator);
        let e = read::execute(
            &api,
            Query::List {
                kind: Listing::Projects,
                build: None,
                page: None,
                size: 50,
                filters: json!({}),
            },
        )
        .unwrap_err();
        assert_eq!(e.details["absence_proven"], false);
        assert_eq!(e.details["items"].as_array().unwrap().len(), 1);
        assert!(
            !serde_json::to_string(&e)
                .unwrap()
                .contains("synthetic-private-token")
        );
        assert_eq!(server.finish().len(), 2);
    }
    let server = Server::new(vec![
        ok(json!([{"id":5,"serverType":0},{"id":6,"serverType":4}])),
        ok(json!([{ "id":"wrong"}])),
    ]);
    let api = api(&server, Client::Main);
    assert_eq!(
        read::execute(&api, Query::Backups(Some(0))).unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        read::execute(
            &api,
            Query::Wa {
                id: "expected".into(),
                build: Some(4)
            }
        )
        .is_err()
    );
    server.finish();
}
