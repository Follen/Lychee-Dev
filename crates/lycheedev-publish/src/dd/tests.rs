use super::*;
use crate::test_support::{Reply, Server};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    time::Duration,
};

#[derive(Clone)]
struct Call {
    url: Url,
    body: Option<Vec<u8>>,
    extra: Vec<(String, String)>,
}
#[derive(Default, Clone)]
pub(super) struct Probe {
    calls: Rc<RefCell<Vec<Call>>>,
    urls: Rc<RefCell<VecDeque<String>>>,
    closed: Rc<Cell<bool>>,
    fail: Rc<Cell<bool>>,
    delay: Rc<Cell<bool>>,
}
impl Signer for Probe {
    fn sign(&mut self, url: &str, body: Option<&[u8]>, extra: &[(&str, &str)]) -> Result<String> {
        let url = Url::parse(url).unwrap();
        self.calls.borrow_mut().push(Call {
            url: url.clone(),
            body: body.map(<[u8]>::to_vec),
            extra: extra
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        });
        if self.fail.get() {
            return Err(native_error("synthetic signing failure".into()));
        }
        if self.delay.get() {
            std::thread::sleep(Duration::from_millis(1100));
        }
        Ok(self.urls.borrow_mut().pop_front().unwrap_or_else(|| {
            let mut url = url;
            url.query_pairs_mut()
                .append_pair("nep_signature", "synthetic-not-a-real-signature");
            url.into()
        }))
    }
    fn close(self: Box<Self>) -> Result<()> {
        self.closed.set(true);
        Ok(())
    }
}
pub(super) fn transport(server: &Server, token: &str) -> (DdTransport, Probe) {
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(10));
    let probe = Probe::default();
    let client = DdTransport {
        http: Http::loopback(&control, server.url.clone()),
        control,
        signer: Some(Box::new(probe.clone())),
        cookie: "synthetic-cookie".into(),
        token: token.into(),
        clock: None,
    };
    (client, probe)
}
fn clock(ts: u64) -> Reply {
    Reply::Json(json!({"code":200,"result":{"ts":ts}}))
}
fn header<'a>(headers: &'a str, name: &str) -> &'a str {
    headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find_map(|(key, value)| key.eq_ignore_ascii_case(name).then_some(value.trim()))
        .unwrap()
}

#[test]
fn borrowed_author_session_reuses_its_original_cookie_and_rebinds_requests_without_desktop_login() {
    use super::session::{BorrowedSession, TaskSession};
    let server = Server::new(vec![
        clock(1793500000),
        Reply::Json(
            json!({"code":200,"result":{"uid":9007199254740993u64,"token":"synthetic-author-token","nickname":"作者"}}),
        ),
        Reply::Json(json!({"code":0,"result":{"read":1}})),
        Reply::Json(json!({"code":0,"result":{"read":2}})),
    ]);
    let (mut client, probe) = transport(&server, "");
    let startup = OperationControl::uncancelable(Instant::now() + Duration::from_millis(300));
    client.begin_operation(&startup).unwrap();
    let mut session = TaskSession::Borrowed(
        BorrowedSession::attach(
            client,
            desktop::Desktop::synthetic(
                lycheedev_windows::ProcessIdentity {
                    pid: 41,
                    created: 42000,
                },
                lycheedev_windows::ProcessIdentity {
                    pid: 43,
                    created: 44000,
                },
            ),
            std::env::temp_dir(),
        )
        .unwrap(),
    );
    std::thread::sleep(Duration::from_millis(320));
    let stopped = OperationControl::default();
    stopped.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(session.begin_operation(&stopped).unwrap_err().exit_code, 7);
    session
        .begin_operation(&OperationControl::default())
        .unwrap();
    assert_eq!(
        session.get("/addon/detail", &[]).unwrap()["result"]["read"],
        1
    );
    session
        .begin_operation(&OperationControl::default())
        .unwrap();
    assert_eq!(
        session.get("/addon/detail", &[]).unwrap()["result"]["read"],
        2
    );
    let identity = session.identity();
    assert_eq!(identity["session_source"], "desktop-cookie");
    assert_eq!(identity["native_login_count"], 0);
    assert_eq!(identity["credential_kind"], Value::Null);
    assert_eq!(identity["logout_requested"], false);
    assert_eq!(identity["author"]["author_id"], "9007199254740993");
    assert!(!identity.to_string().contains("synthetic-cookie"));
    assert!(!identity.to_string().contains("synthetic-author-token"));
    session.close().unwrap();
    assert!(probe.closed.get());
    let requests = server.finish();
    assert_eq!(requests.len(), 4);
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("GET /login/dflogin"))
            .count(),
        1
    );
    for request in &requests[1..] {
        assert_eq!(
            header(&request.headers, "cookie"),
            "NTES_YD_SESS=synthetic-cookie"
        );
    }
}
#[test]
fn rejected_borrowed_author_authentication_releases_its_signer_without_echoing_the_cookie() {
    let server = Server::new(vec![
        clock(1793500000),
        Reply::Json(json!({"code":401,
        "msg":"synthetic-cookie synthetic-author-token must never escape"})),
    ]);
    let (client, probe) = transport(&server, "");
    let error = super::session::BorrowedSession::attach(
        client,
        desktop::Desktop::synthetic(
            lycheedev_windows::ProcessIdentity {
                pid: 41,
                created: 42000,
            },
            lycheedev_windows::ProcessIdentity {
                pid: 43,
                created: 44000,
            },
        ),
        std::env::temp_dir(),
    )
    .err()
    .unwrap();
    assert_eq!(error.exit_code, 5);
    assert!(probe.closed.get());
    let encoded = serde_json::to_string(&error).unwrap();
    assert!(!encoded.contains("synthetic-cookie"));
    assert!(!encoded.contains("synthetic-author-token"));
    assert_eq!(server.finish().len(), 2);
}

#[test]
fn explicit_login_and_serial_requests_bind_exact_body_bytes_and_one_timestamp_sample() {
    let server = Server::new(vec![
        clock(1793500000),
        Reply::Json(
            json!({"code":200,"result":{"uid":9007199254740993u64,"token":"synthetic-author-token","nickname":"作者"}}),
        ),
        Reply::Json(json!({"code":0,"result":{"items":[]}})),
        Reply::Json(json!({"code":0,"result":{"saved":true}})),
    ]);
    let (mut client, probe) = transport(&server, "");
    assert_eq!(client.get("/addon/detail", &[]).unwrap_err().exit_code, 3);
    let author = client.login().unwrap();
    assert_eq!(author["author_id"], "9007199254740993");
    assert!(!author.to_string().contains("token"));
    assert!(client.login().is_err());
    client
        .get("/addon/detail", &[("sn", "插件&空 格+?".into())])
        .unwrap();
    let body = json!({"name":"中文😀","count":0,"enabled":false,"clear":null});
    let expected = serde_json::to_vec(&body).unwrap();
    client.post("/addon/update", &[], &body, true).unwrap();
    client.close().unwrap();
    assert!(probe.closed.get());
    let requests = server.finish();
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[0].line, "GET /server/ts HTTP/1.1");
    assert!(!requests[0].headers.to_ascii_lowercase().contains("cookie:"));
    let calls = probe.calls.borrow();
    assert_eq!(calls.len(), 3);
    for (call, request) in calls.iter().zip(&requests[1..]) {
        assert_eq!(call.extra[1].1, header(&request.headers, "x-timestamp"));
        assert_eq!(call.extra[0].1, header(&request.headers, "x-w163-token"));
        assert_eq!(
            header(&request.headers, "cookie"),
            "NTES_YD_SESS=synthetic-cookie"
        );
        assert_eq!(header(&request.headers, "user-agent"), USER_AGENT_VALUE);
        assert_eq!(
            header(&request.headers, "content-type"),
            "application/json;charset=UTF-8"
        );
        assert_eq!(
            call.url.origin().ascii_serialization(),
            "https://uiapi.w.163.com"
        );
        assert!(request.line.contains("nep_signature=synthetic"));
    }
    assert_eq!(calls[0].extra[0].1, "");
    assert_eq!(calls[1].extra[0].1, "synthetic-author-token");
    assert_eq!(
        calls[1]
            .url
            .query_pairs()
            .find(|(key, _)| key == "sn")
            .unwrap()
            .1,
        "插件&空 格+?"
    );
    assert_eq!(calls[2].body.as_deref(), Some(expected.as_slice()));
    assert_eq!(requests[3].body, expected);
}

#[test]
fn crossing_a_second_during_native_signing_keeps_header_and_signature_clock_identical() {
    let server = Server::new(vec![
        clock(1793500000),
        Reply::Json(json!({"code":0,"result":{}})),
    ]);
    let (mut client, probe) = transport(&server, "synthetic-token");
    probe.delay.set(true);
    client.get("/game/list", &[]).unwrap();
    client.close().unwrap();
    let requests = server.finish();
    let calls = probe.calls.borrow();
    assert_eq!(calls[0].extra[1].1, "1793500000");
    assert_eq!(header(&requests[1].headers, "x-timestamp"), "1793500000");
}

#[test]
fn only_exact_get_signature_rejection_permits_one_resign_without_another_login() {
    let server = Server::new(vec![
        clock(1793500000),
        Reply::Json(json!({"code":409,"msg":"签名无效"})),
        clock(1793500100),
        Reply::Json(json!({"code":0,"result":{}})),
        Reply::Json(json!({"code":409,"msg":"other failure synthetic-token"})),
    ]);
    let (mut client, probe) = transport(&server, "synthetic-token");
    client.get("/game/list", &[]).unwrap();
    let error = client.get("/game/list", &[]).unwrap_err();
    assert_eq!(error.details["business_code"], 409);
    assert!(!error.to_string().contains("synthetic-token"));
    client.close().unwrap();
    let requests = server.finish();
    assert_eq!(requests.len(), 5);
    assert!(requests.iter().all(|r| !r.line.contains("dflogin")));
    let calls = probe.calls.borrow();
    assert_eq!(calls[0].extra[1].1, "1793500000");
    assert_eq!(calls[1].extra[1].1, "1793500100");

    let server = Server::new(vec![
        clock(1),
        Reply::Json(json!({"code":409,"msg":"签名无效"})),
        clock(2),
        Reply::Json(json!({"code":409,"msg":"签名无效"})),
    ]);
    let (mut client, probe) = transport(&server, "synthetic-token");
    assert_eq!(
        client.get("/game/list", &[]).unwrap_err().details["business_code"],
        409
    );
    client.close().unwrap();
    assert_eq!(server.finish().len(), 4);
    assert_eq!(probe.calls.borrow().len(), 2);
}

#[test]
fn post_rejection_and_lost_or_malformed_response_never_replay_or_relogin() {
    for reply in [
        Reply::Json(json!({"code":409,"msg":"签名无效 synthetic-token"})),
        Reply::Drop,
        Reply::Json(json!(["unexpected"])),
        Reply::Json(json!({"code":"0","result":{}})),
    ] {
        let server = Server::new(vec![clock(1), reply]);
        let (mut client, probe) = transport(&server, "synthetic-token");
        let error = client
            .post("/addon/update", &[], &json!({"name":"original"}), true)
            .unwrap_err();
        assert_eq!(error.exit_code, 5);
        assert!(matches!(
            error.details["submission"].as_str(),
            Some("rejected" | "uncertain")
        ));
        assert_eq!(error.details["replay_allowed"], false);
        assert!(
            !serde_json::to_string(&error)
                .unwrap()
                .contains("synthetic-token")
        );
        client.close().unwrap();
        let requests = server.finish();
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.line.starts_with("POST "))
                .count(),
            1
        );
        assert_eq!(probe.calls.borrow().len(), 1);
    }
    let server = Server::new(vec![
        clock(1),
        Reply::Json(json!({"code":409,"msg":"签名无效"})),
    ]);
    let (mut client, _) = transport(&server, "synthetic-token");
    client
        .post(
            "/addon/addon_list",
            &[],
            &json!({"origin":"created"}),
            false,
        )
        .unwrap_err();
    client.close().unwrap();
    assert_eq!(server.finish().len(), 2);
}

#[test]
fn signed_target_cannot_move_author_credentials_or_change_selected_arguments() {
    let original = route(
        "/addon/detail",
        &[
            ("sn", "original".into()),
            ("n", "1".into()),
            ("n", "1".into()),
        ],
    )
    .unwrap();
    assert!(
        signed_target(
            &original,
            "https://uiapi.w.163.com/addon/detail?n=1&sn=original&n=1&sig=x"
        )
        .is_ok()
    );
    for target in [
        "https://attacker.invalid/addon/detail?sn=original&n=1&n=1",
        "http://uiapi.w.163.com/addon/detail?sn=original&n=1&n=1",
        "https://uiapi.w.163.com:444/addon/detail?sn=original&n=1&n=1",
        "https://user@uiapi.w.163.com/addon/detail?sn=original&n=1&n=1",
        "https://uiapi.w.163.com/addon/update?sn=original&n=1&n=1",
        "https://uiapi.w.163.com/addon/detail?sn=other&n=1&n=1",
        "https://uiapi.w.163.com/addon/detail?sn=original&n=1",
        "https://uiapi.w.163.com/addon/detail?sn=original&sn=other&n=1&n=1",
        "https://uiapi.w.163.com/addon/detail?sn=original&n=1&n=1#secret",
    ] {
        assert!(signed_target(&original, target).is_err(), "{target}");
    }
    let server = Server::new(vec![]);
    let (mut client, probe) = transport(&server, "synthetic-token");
    client.clock = Some(Clock {
        timestamp: 1,
        sampled: Instant::now(),
    });
    probe
        .urls
        .borrow_mut()
        .push_back("https://attacker.invalid/addon/update".into());
    let error = client
        .post("/addon/update", &[], &json!({}), true)
        .unwrap_err();
    assert_eq!(error.details["submission"], "not_sent");
    client.close().unwrap();
    assert!(server.finish().is_empty());
}

#[test]
fn clock_and_signature_failures_stop_before_submission_without_local_time_or_unsigned_fallback() {
    for reply in [
        Reply::Json(json!({"code":200,"result":{"ts":"bad"}})),
        Reply::Drop,
        Reply::Json(json!({"code":0,"result":{"ts":1}})),
    ] {
        let server = Server::new(vec![reply]);
        let (mut client, probe) = transport(&server, "synthetic-token");
        let error = client
            .post("/addon/update", &[], &json!({}), true)
            .unwrap_err();
        assert_eq!(error.details["submission"], "not_sent");
        assert_eq!(error.details["replay_allowed"], false);
        assert!(probe.calls.borrow().is_empty());
        client.close().unwrap();
        assert_eq!(server.finish().len(), 1);
    }
    let server = Server::new(vec![clock(1)]);
    let (mut client, probe) = transport(&server, "synthetic-token");
    probe.fail.set(true);
    assert_eq!(
        client
            .post("/addon/update", &[], &json!({}), true)
            .unwrap_err()
            .details["submission"],
        "not_sent"
    );
    client.close().unwrap();
    assert_eq!(server.finish().len(), 1);
}

#[test]
fn author_headers_never_follow_http_redirects_and_invalid_inputs_are_local_only() {
    let server = Server::new(vec![
        clock(1),
        Reply::Raw(
            "HTTP/1.1 302 Found\r\nLocation: https://attacker.invalid/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ),
    ]);
    let (mut client, _) = transport(&server, "synthetic-token");
    let error = client
        .post("/addon/update", &[], &json!({}), true)
        .unwrap_err();
    assert_eq!(error.details["http_status"], 302);
    assert_eq!(error.details["submission"], "uncertain");
    client.close().unwrap();
    assert_eq!(server.finish().len(), 2);
    for cookie in ["", "a; injected=token", "a\r\nX: token", "\0secret"] {
        assert!(validate_credentials(cookie, "synthetic-token").is_err());
    }
    let server = Server::new(vec![]);
    let (mut client, probe) = transport(&server, "synthetic-token");
    for path in [
        "https://attacker.invalid/",
        "//attacker.invalid/x",
        "/../other",
        "/%2e%2e/other",
        "/x?token=secret",
        "/x\\other",
    ] {
        assert_eq!(
            client
                .post(path, &[], &json!({}), true)
                .unwrap_err()
                .details["submission"],
            "not_sent"
        );
    }
    client.close().unwrap();
    assert!(server.finish().is_empty());
    assert!(probe.calls.borrow().is_empty());
}
