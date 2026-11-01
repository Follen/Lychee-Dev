use super::*;
use crate::test_support::{Reply, Server};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use managed::{Reader, tests::Fixture};
use serde_json::json;
use std::cell::Cell;

fn token(claims: Value) -> String {
    format!(
        "{}.{}.testSignature",
        URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256"}"#),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
    )
}
fn claims(expiry: u64) -> Value {
    json!({"appid":"webcc","exp":expiry,"user":{"uid":"18446744073709551615","eid":"100000000000000000000000000001"}})
}
fn header<'a>(request: &'a crate::test_support::Request, name: &str) -> Option<&'a str> {
    request
        .headers
        .lines()
        .filter_map(|s| s.split_once(':'))
        .find_map(|(k, v)| k.eq_ignore_ascii_case(name).then_some(v.trim()))
}
#[test]
fn gui_cached_jwt_stays_inside_reader_and_only_inherited_deduplicated_choices_leave_http() {
    let server = Server::new(vec![Reply::Json(json!({"code":"OK","data":{"list":[
        {"teamId":9007199254740993u64,"teamName":"中文房间","token":"never-return-me","channelList":[
            {"channelId":"01","channelType":0,"name":"聊天"},
            {"channel_id":"01","channel_type":0,"name":"duplicate"},
            {"channelId":"02","channelType":"text","channelName":null}
        ]}
    ]}}))]);
    let control = OperationControl::default();
    let http = Http::loopback(&control, server.url.clone());
    let mut memory = Fixture::new();
    let base = memory.base;
    let checks = Cell::new(0);
    let value = gui_with(
        &mut memory,
        base,
        "1234",
        &http,
        &control,
        || {
            checks.set(checks.get() + 1);
            Ok(())
        },
        || 2_000_000_000.0,
    )
    .unwrap()
    .value()
    .unwrap();
    assert!(checks.get() >= 4);
    assert_eq!(
        value,
        json!({"total":3,"items":[
        {"room_id":"9007199254740993","room_name":"中文房间","channel_id":"","channel_name":null,"channel_type":""},
        {"room_id":"9007199254740993","room_name":"中文房间","channel_id":"01","channel_name":"聊天","channel_type":"0"},
        {"room_id":"9007199254740993","room_name":"中文房间","channel_id":"02","channel_name":null,"channel_type":"text"}
    ],"dependencies":[{"parent":"room_id","children":["channel_id","channel_type"]}]})
    );
    let requests = server.finish();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].line,
        "GET /v1/mixteammsgproxy/channelList?source=pluginPublish HTTP/1.1"
    );
    assert!(
        header(&requests[0], "authentication")
            .unwrap()
            .ends_with(".fixtureSignature")
    );
    assert_eq!(
        header(&requests[0], "user-agent"),
        Some(super::super::USER_AGENT_VALUE)
    );
    assert_eq!(header(&requests[0], "accept"), Some("application/json"));
    for key in ["authorCookie", "authorToken", "cookie", "authorization"] {
        assert_eq!(header(&requests[0], key), None);
    }
    assert!(requests[0].body.is_empty());
}
#[test]
fn absent_cache_refreshes_once_with_original_identity_clock_and_signature_without_gui_mutation() {
    let fresh = token(claims(2_000_000_600));
    let server = Server::new(vec![
        Reply::Json(json!({"code":200,"data":{"jwt":fresh}})),
        Reply::Json(json!({"code":0,"data":[]})),
    ]);
    let control = OperationControl::default();
    let http = Http::loopback(&control, server.url.clone());
    let mut memory = Fixture::new();
    let base = memory.base;
    memory.pointer(
        memory.label("helper_cached_slot"),
        memory.label("null_token"),
    );
    let result = gui_with(
        &mut memory,
        base,
        "1234",
        &http,
        &control,
        || Ok(()),
        || 2_000_000_000.75,
    )
    .unwrap()
    .value()
    .unwrap();
    assert_eq!(result["total"], 0);
    let snapshot = Reader::new(&mut memory, &control).snapshot(base).unwrap();
    assert!(snapshot.cached.is_empty());
    let requests = server.finish();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].line,
        "GET /v1/jwt?appid=webcc&uid=18446744073709551615&eid=100000000000000000000000000001&timestamp=1999999997 HTTP/1.1"
    );
    assert_eq!(
        header(&requests[0], "authentication"),
        Some("d237fca60e05d083f3a5cebd86a35099")
    );
    assert_eq!(header(&requests[1], "authentication"), Some(fresh.as_str()));
}
#[test]
fn only_expired_matching_cache_can_refresh_and_malformed_or_foreign_identity_never_sends_http() {
    let mut memory = Fixture::new();
    let base = memory.base;
    let control = OperationControl::default();
    let mut snapshot = Reader::new(&mut memory, &control).snapshot(base).unwrap();
    let server = Server::new(vec![]);
    let http = Http::loopback(&control, server.url.clone());
    let mut wrong = claims(1_999_999_999);
    wrong["user"]["uid"] = json!("other-account");
    let mut boolean = claims(2_000_000_600);
    boolean["exp"] = json!(true);
    for candidate in [
        "not-a-jwt".into(),
        token(wrong),
        token(boolean),
        token(claims(2_000_001_201)),
        format!(
            "{}.{}.s",
            URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#),
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims(2_000_000_600)).unwrap())
        ),
        format!(
            "{}.{}.s",
            URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256","alg":"HS256"}"#),
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims(2_000_000_600)).unwrap())
        ),
    ] {
        snapshot.cached = candidate.clone();
        let error = jwt::current(&snapshot, &http, 2_000_000_000.0).unwrap_err();
        assert_eq!(error.exit_code, 3);
        assert!(!error.to_string().contains(&candidate));
    }
    assert!(server.finish().is_empty());
    snapshot.cached = token(claims(1_999_999_999));
    let fresh = token(claims(2_000_000_600));
    let server = Server::new(vec![Reply::Json(json!({"code":"OK","data":{"jwt":fresh}}))]);
    let http = Http::loopback(&control, server.url.clone());
    assert_eq!(
        jwt::current(&snapshot, &http, 2_000_000_000.0).unwrap(),
        fresh
    );
    assert_eq!(server.finish().len(), 1);
    let mut string_user = claims(2_000_000_600);
    string_user["user"] = json!(serde_json::to_string(&string_user["user"]).unwrap());
    assert!(jwt::validate(&token(string_user), &snapshot, 2_000_000_000.0).unwrap());
}
#[test]
fn account_or_operation_changes_stop_before_channel_query_and_post_http_changes_discard_choices() {
    for at in [0, 2, 3] {
        let control = OperationControl::default();
        let server = Server::new(vec![]);
        let http = Http::loopback(&control, server.url.clone());
        let mut memory = Fixture::new();
        let base = memory.base;
        let count = Cell::new(0);
        let result = gui_with(
            &mut memory,
            base,
            if at == 0 { "other-account" } else { "1234" },
            &http,
            &control,
            || {
                count.set(count.get() + 1);
                if count.get() == at {
                    Err(fault("synthetic profile changed"))
                } else {
                    Ok(())
                }
            },
            || 2_000_000_000.0,
        );
        assert!(result.is_err());
        assert!(server.finish().is_empty());
    }
    let control = OperationControl::default();
    let cancel = control.tracked_cancel_handle().unwrap();
    let server = Server::with_hook(
        vec![Reply::Json(json!({"code":"OK","data":[]}))],
        move |_| {
            cancel.cancel();
        },
    );
    let http = Http::loopback(&control, server.url.clone());
    let mut memory = Fixture::new();
    let base = memory.base;
    assert_eq!(
        gui_with(
            &mut memory,
            base,
            "1234",
            &http,
            &control,
            || Ok(()),
            || 2_000_000_000.0
        )
        .err()
        .unwrap()
        .exit_code,
        7
    );
    assert_eq!(server.finish().len(), 1);
}
#[test]
fn rejected_redirected_unknown_and_credential_echo_responses_are_never_empty_success_or_retried() {
    for reply in [
        Reply::Json(json!({"code":"REJECTED","data":[],"message":"private-server-body"})),
        Reply::Json(json!({"code":"OK","data":{"unexpected":[]}})),
        Reply::Json(json!({"code":"OK","data":[{"teamId":"room","teamName":"synthetic-jwt"}]})),
        Reply::Raw(
            "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/leak\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ),
        Reply::Drop,
    ] {
        let server = Server::new(vec![reply]);
        let control = OperationControl::default();
        let http = Http::loopback(&control, server.url.clone());
        let error = query(&http, "synthetic-jwt").err().unwrap();
        assert!(!error.to_string().contains("private-server-body"));
        assert!(!error.to_string().contains("synthetic-jwt"));
        assert_eq!(server.finish().len(), 1);
    }
    let valid = projection::choices(&json!({"data":[]})).unwrap();
    let mut encoded = serde_json::to_value(valid).unwrap();
    encoded["token"] = json!("private");
    assert!(serde_json::from_value::<Choices>(encoded).is_err());
    for data in [
        json!({}),
        json!([false]),
        json!([{"teamId":"r","children":{}}]),
    ] {
        assert!(projection::choices(&json!({"data":data})).is_err());
    }
}
