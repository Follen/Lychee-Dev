use super::*;
use crate::{
    test_support::{Reply, Server, TestDir},
    transport::Http,
};
use std::time::Duration;
fn control() -> OperationControl {
    OperationControl::uncancelable(std::time::Instant::now() + Duration::from_secs(20))
}
fn open(root: &Path, server: &Server, control: &OperationControl) -> Auth {
    let mut auth = Auth::open(root, control).unwrap();
    auth.api.http = Http::loopback(control, server.url.clone());
    auth
}
fn session(uid: &str) -> Value {
    json!({"profile":{"heybox_id":uid},"pkey":"synthetic-private-key","expire_at":now()+3600})
}
fn ok(result: Value) -> Reply {
    Reply::Json(json!({"status":"ok","result":result}))
}
fn page() -> Reply {
    Reply::Bytes(br#"<html><meta content="test-build" name="web_version"></html>"#.to_vec())
}
fn state(root: &Path) -> State {
    let cipher = fs::read(root.join("publish/blackbox/auth/session.dpapi")).unwrap();
    assert!(
        !cipher
            .windows(b"synthetic-private-key".len())
            .any(|x| x == b"synthetic-private-key")
    );
    serde_json::from_slice(&credentials::unprotect_current_user(&cipher, Some(ENTROPY)).unwrap())
        .unwrap()
}
#[test]
fn sms_uses_native_encryption_and_recovers_consumed_code_by_reads_without_changing_account() {
    let temp = TestDir::new();
    let c = control();
    let server = Server::new(vec![
        page(),
        ok(json!({})),
        ok(session("123")),
        Reply::Drop,
        ok(session("123")),
        ok(json!({"moduleList":[]})),
        ok(json!({"backupList":[]})),
        ok(session("999")),
    ]);
    let started = open(&temp.0, &server, &c)
        .finish(|a| a.start("sms", Some("13812345678")))
        .unwrap();
    let id = started["login_id"].as_str().unwrap();
    assert_eq!(started["status"], "waiting_code");
    let original_expiry = state(&temp.0).pending.unwrap().expires_at;
    let error = open(&temp.0, &server, &c)
        .finish(|a| a.submit(id, "123456"))
        .unwrap_err();
    assert_eq!(error.details["login_id"], id);
    assert!(!error.to_string().contains("123456"));
    let pending = state(&temp.0).pending.unwrap();
    assert!(pending.candidate.is_some());
    assert_eq!(pending.expires_at, original_expiry);
    let authenticated = open(&temp.0, &server, &c).finish(|a| a.status(id)).unwrap();
    assert_eq!(authenticated["account_id"], "123");
    assert_eq!(authenticated["authenticated"], true);
    let mismatch = open(&temp.0, &server, &c)
        .finish(|a| a.renew(false).map(|s| authenticated_value(&s)))
        .unwrap_err();
    assert_eq!(mismatch.code, "publish.blackbox_identity");
    assert_eq!(
        state(&temp.0).sessions["sms"].identity.as_deref(),
        Some("123")
    );
    let requests = server.finish();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.contains("/account/login_code/"))
            .count(),
        1
    );
    let url = Url::parse(&format!(
        "https://local.invalid/?{}",
        String::from_utf8(requests[1].body.clone()).unwrap()
    ))
    .unwrap();
    let phone = url
        .query_pairs()
        .find(|(k, _)| k == "phone_num")
        .unwrap()
        .1
        .into_owned();
    assert_eq!(STANDARD.decode(phone).unwrap().len(), 128);
    assert!(!String::from_utf8_lossy(&requests[1].body).contains("13812345678"));
    assert!(requests[0].line.contains("origin=heybox_open"));
    assert!(requests[1].line.contains("web_version=test-build"));
}
fn authenticated_value(s: &Session) -> Value {
    json!({"account_id":s.identity})
}
#[test]
fn lost_sms_response_retains_original_deadline_and_never_resends_or_switches_method() {
    let temp = TestDir::new();
    let c = control();
    let server = Server::new(vec![page(), Reply::Drop]);
    let error = open(&temp.0, &server, &c)
        .finish(|a| a.start("sms", Some("13812345678")))
        .unwrap_err();
    let id = error.details["login_id"].as_str().unwrap();
    let status = open(&temp.0, &server, &c).finish(|a| a.status(id)).unwrap();
    assert_eq!(status["status"], "sms_send_uncertain");
    let error = open(&temp.0, &server, &c)
        .finish(|a| a.start("wechat", None))
        .unwrap_err();
    assert_eq!(error.exit_code, 6);
    open(&temp.0, &server, &c).finish(|a| a.forget()).unwrap();
    assert!(state(&temp.0).pending.is_none());
    assert_eq!(server.finish().len(), 2);
}
#[test]
fn wechat_qr_polling_preserves_oauth_state_and_uses_only_fixed_redirects() {
    let temp = TestDir::new();
    let c = control();
    let server=Server::new(vec![page(),Reply::Raw("HTTP/1.1 302 Found\r\nLocation: https://open.weixin.qq.com/connect/qrconnect?appid=fixture&redirect_uri=https%3A%2F%2Fapi.xiaoheihe.cn%2Faccount%2Fwechat%2Flogin_redirect%2Fv2%2Fweb_sso%2F&state=synthetic-state#wechat_redirect\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
        Reply::Bytes(br#"<img src="/connect/qrcode/fixture_uuid"><script>var fordevtool = 'https://long.open.weixin.qq.com/connect/l/qrconnect?uuid=fixture_uuid'</script>"#.to_vec()),
        Reply::Bytes(b"\x89PNG\r\n\x1a\nsynthetic-image".to_vec()),Reply::Bytes(b"window.wx_errcode=405;window.wx_code='synthetic-code';".to_vec()),
        Reply::Raw("HTTP/1.1 302 Found\r\nLocation: https://login.xiaoheihe.cn/\r\nSet-Cookie: oauth_probe=synthetic-cookie; Path=/; Secure; HttpOnly\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
        Reply::Bytes(b"finished".to_vec()),ok(session("456")),ok(session("456")),ok(json!({"moduleList":[]})),ok(json!({"backupList":[]}))]);
    let started = open(&temp.0, &server, &c)
        .finish(|a| a.start("wechat", None))
        .unwrap();
    let id = started["login_id"].as_str().unwrap();
    let image = PathBuf::from(started["image_file"].as_str().unwrap());
    assert!(image.exists());
    assert_eq!(started["status"], "waiting_scan");
    let done = open(&temp.0, &server, &c).finish(|a| a.status(id)).unwrap();
    assert_eq!(done["authenticated"], true);
    assert_eq!(done["method"], "wechat");
    assert!(!image.exists());
    let requests = server.finish();
    assert!(
        requests[5]
            .line
            .contains("code=synthetic-code&state=synthetic-state")
    );
    assert!(!requests[6].headers.contains("oauth_probe")); // Host-only API cookie never reaches login host.
    assert!(requests[7].headers.contains("oauth_probe=synthetic-cookie"));
    assert!(!done.to_string().contains("synthetic"));
}
#[test]
fn lost_wechat_exchange_is_not_replayed_and_forget_does_not_select_another_saved_login() {
    let temp = TestDir::new();
    let c = control();
    let server = Server::new(vec![Reply::Drop, ok(json!({}))]);
    let mut auth = open(&temp.0, &server, &c);
    let stamp = now();
    let mut p=Pending {id:"a".repeat(32),created_at:stamp,expires_at:stamp+TTL,status:"waiting_confirmation".into(),session:Session {method:"wechat".into(),device_id:"fixture".into(),..Default::default()},phone_encrypted:None,candidate:None,image_suffix:None,oauth_uuid:Some("fixture".into()),oauth_callback:None,oauth_state:None,oauth_next:Some("https://api.xiaoheihe.cn/account/wechat/login_redirect/v2/web_sso/?code=consumed&state=fixture".into()),oauth_last:None,callback_attempted:false};
    auth.save_pending(&p).unwrap();
    assert!(auth.wechat_finish(&mut p).is_err());
    auth.finish(|_| Ok(json!({}))).unwrap();
    // The only follow-up request is the state read. No callback request is sent twice.
    let failed = open(&temp.0, &server, &c).finish(|a| a.status(&"a".repeat(32)));
    assert!(failed.is_err());
    let requests = server.finish();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].line.contains("/account/wechat/state/"));
    let mut auth = Auth::open(&temp.0, &c).unwrap();
    for (method, id) in [("sms", "123"), ("wechat", "456")] {
        let mut s = Session {
            method: method.into(),
            ..Default::default()
        };
        s.apply(&session(id), None).unwrap();
        auth.state.sessions.insert(method.into(), s);
    }
    auth.state.active = Some("wechat".into());
    auth.save().unwrap();
    auth.finish(|a| a.forget()).unwrap();
    let saved = state(&temp.0);
    assert!(saved.active.is_none());
    assert!(saved.sessions.contains_key("sms"));
    assert!(!saved.sessions.contains_key("wechat"));
}
