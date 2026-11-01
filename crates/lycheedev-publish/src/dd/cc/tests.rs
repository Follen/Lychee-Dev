use super::*;
use crate::dd::CredentialKind;
use crate::test_support::{Reply, Server};
use flate2::{Compression, write::ZlibEncoder};
use std::{
    io::Write,
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Happy,
    RejectHandshake,
    RejectLogin,
    DropLogin,
    WrongCorrelation,
    Kick,
    KickOnHeartbeat,
    HoldHeartbeat,
    HoldToken,
}

#[test]
fn channel_operations_obtain_jwt_and_choices_on_the_same_original_cc_connection_without_another_login()
 {
    let peer = Peer::new(Mode::Happy);
    let control = OperationControl::default();
    let mut session = start(
        &peer,
        CredentialKind::Email,
        &control,
        Duration::from_millis(20),
    )
    .unwrap();
    let server = Server::new(vec![
        Reply::Json(json!({"code":"OK","data":{"jwt":"synthetic-channel-jwt-one"}})),
        Reply::Json(
            json!({"code":"OK","data":[{"teamId":"first","channels":[{"channelId":"001","name":"频道"}]}]}),
        ),
        Reply::Json(json!({"code":"OK","data":{"jwt":"synthetic-channel-jwt-two"}})),
        Reply::Json(json!({"code":200,"data":[]})),
    ]);
    let http = Http::loopback(&control, server.url.clone());
    assert_eq!(
        crate::dd::channel::native_with(&mut session, &http, &control).unwrap()["total"],
        2
    );
    assert_eq!(
        crate::dd::channel::native_with(&mut session, &http, &control).unwrap()["total"],
        0
    );
    session.close().unwrap();
    let requests = server.finish();
    assert_eq!(requests.len(), 4);
    for (index, token) in [
        (1, "synthetic-channel-jwt-one"),
        (3, "synthetic-channel-jwt-two"),
    ] {
        assert_eq!(
            requests[index].line,
            "GET /v1/mixteammsgproxy/channelList?source=pluginPublish HTTP/1.1"
        );
        let headers = requests[index].headers.to_lowercase();
        assert!(headers.contains(&format!("authentication: {token}\r\n")));
        assert!(!headers.contains("authorcookie:"));
        assert!(!headers.contains("authortoken:"));
    }
    let packets = peer.finish();
    assert_eq!(packets.iter().filter(|r| r.command == 3).count(), 1);
    assert_eq!(packets.iter().filter(|r| r.command == 8).count(), 2);
}
struct Seen {
    command: u16,
    body: Value,
}
struct Peer {
    url: String,
    seen: Arc<Mutex<Vec<Seen>>>,
    closed: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
fn decode_request(bytes: &[u8]) -> (u16, String, Value) {
    // Independent reader for the reference <HH12xH,prop,H,data> layout. Do not
    // use the production encoder/response reader to interpret client packets.
    assert_eq!(&bytes[..2], &[0xac, 0xac]);
    assert_eq!(&bytes[4..16], &[0; 12]);
    let command = u16::from_le_bytes(bytes[2..4].try_into().unwrap());
    let size = u16::from_le_bytes(bytes[16..18].try_into().unwrap()) as usize;
    let prop: Value = serde_json::from_slice(&bytes[18..18 + size]).unwrap();
    let ext: Value = serde_json::from_str(prop["extInfo"].as_str().unwrap()).unwrap();
    let offset = 18 + size;
    let size = u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap()) as usize;
    assert_eq!(bytes.len(), offset + 2 + size);
    let body = serde_json::from_slice(&bytes[offset + 2..]).unwrap();
    (command, ext["seq"].as_str().unwrap().into(), body)
}
fn response(sid: u16, command: u16, sequence: &str, code: u32, data: &Value) -> Vec<u8> {
    let prop = serde_json::to_vec(&json!({"extInfo":json!({"seq":sequence}).to_string()})).unwrap();
    let data = serde_json::to_vec(data).unwrap();
    let mut out = Vec::new();
    out.extend_from_slice(&sid.to_le_bytes());
    out.extend_from_slice(&command.to_le_bytes());
    out.extend_from_slice(&[0; 12]);
    for (index, bytes) in [
        prop.as_slice(),
        b"rejected synthetic-cookie private-token",
        data.as_slice(),
    ]
    .into_iter()
    .enumerate()
    {
        if index == 1 {
            out.extend_from_slice(&code.to_le_bytes());
        }
        out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(bytes);
    }
    out
}
impl Peer {
    fn new(mode: Mode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("ws://{}/", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let observed = seen.clone();
        let closed = Arc::new(AtomicBool::new(false));
        let ended = closed.clone();
        let worker = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move{
                let listener=tokio::net::TcpListener::from_std(listener).unwrap();
                let (stream,_)=tokio::time::timeout(Duration::from_secs(5),listener.accept()).await.unwrap().unwrap();
                let mut socket=tokio_tungstenite::accept_async(stream).await.unwrap();
                loop{
                    let message=tokio::time::timeout(Duration::from_secs(5),socket.next()).await.unwrap();
                    let bytes=match message{
                        Some(Ok(Message::Binary(bytes)))=>bytes,
                        Some(Ok(Message::Close(_)))=>{let _=socket.close(None).await;break;},
                        None|Some(Err(_))=>break,
                        _=>panic!("unexpected client WebSocket message"),
                    };
                    let (command,seq,body)=decode_request(&bytes);
                    observed.lock().unwrap().push(Seen{command,body});
                    if mode==Mode::DropLogin&&command==3 {break;}
                    if mode==Mode::HoldHeartbeat&&command==4 {continue;}
                    if mode==Mode::HoldToken&&command==8 {continue;}
                    let data=match command{
                        1|4=>json!({}),
                        3=>json!({"uid":9007199254740993u64,"eid":0,"cookieName":"NTES_YD_SESS","cookieValue":"synthetic-cookie","rcToken":"private-unused-rc-token"}),
                        8=>json!({"token":"synthetic-cc-token","nowTime":1793500000000u64}),
                        _=>panic!("unexpected CC command {command}"),
                    };
                    let code=if (mode==Mode::RejectHandshake&&command==1)||(mode==Mode::RejectLogin&&command==3){403}else{0};
                    if mode==Mode::WrongCorrelation{
                        socket.send(Message::Binary(response(protocol::SID,command,"another-task",0,&json!({"cookieValue":"foreign-cookie"})).into())).await.unwrap();
                        socket.send(Message::Binary(response(protocol::SID+1,command,&seq,0,&json!({"cookieValue":"foreign-cookie"})).into())).await.unwrap();
                    }
                    if socket.send(Message::Binary(response(protocol::SID,command,&seq,code,&data).into())).await.is_err(){break;}
                    if (mode==Mode::Kick&&command==3)||(mode==Mode::KickOnHeartbeat&&command==4){
                        let _=socket.send(Message::Binary(response(protocol::SID,5,"push",0,&json!({"reason":"private-kick-reason"})).into())).await;
                    }
                }
                ended.store(true,Ordering::Release);
            });
        });
        Self {
            url,
            seen,
            closed,
            worker: Some(worker),
        }
    }
    fn finish(mut self) -> Vec<Seen> {
        self.worker.take().unwrap().join().unwrap();
        assert!(self.closed.load(Ordering::Acquire));
        Arc::try_unwrap(self.seen)
            .ok()
            .unwrap()
            .into_inner()
            .unwrap()
    }
}
fn credential(kind: CredentialKind) -> SavedCredential {
    SavedCredential {
        kind,
        account: "selected@synthetic.invalid".into(),
        token: "synthetic-original-token".into(),
    }
}
fn device() -> Device {
    Device {
        client_no: "0123456789abcdef0123456789abcdef".into(),
        mac: "00:11:22:33:44:55".into(),
        os_version: "synthetic-os".into(),
        bi_desc: json!({"source":"synthetic-test"}),
    }
}
fn start(
    peer: &Peer,
    kind: CredentialKind,
    control: &OperationControl,
    heartbeat: Duration,
) -> Result<CcSession> {
    CcSession::start_at(
        credential(kind),
        "synthetic-appid".into(),
        device(),
        control,
        peer.url.clone(),
        heartbeat,
    )
}

#[test]
fn one_typed_native_login_preserves_device_fields_and_keeps_heartbeats_independent_of_the_caller() {
    for kind in [CredentialKind::Email, CredentialKind::Mobile] {
        let peer = Peer::new(Mode::Happy);
        let control = OperationControl::default();
        let session = start(&peer, kind, &control, Duration::from_millis(20)).unwrap();
        assert_eq!(session.author_cookie(), "synthetic-cookie");
        let public = session.identity().to_string();
        for secret in [
            "selected@synthetic.invalid",
            "synthetic-original-token",
            "synthetic-cookie",
            "private-unused-rc-token",
        ] {
            assert!(!public.contains(secret));
        }
        assert_eq!(session.identity()["cc_uid"], 9007199254740993u64);
        assert_eq!(session.identity()["native_login_count"], 1);
        thread::sleep(Duration::from_millis(130));
        session.close().unwrap();
        let requests = peer.finish();
        assert_eq!(requests[0].command, 1);
        assert_eq!(
            requests[0].body,
            json!({"mac":"00:11:22:33:44:55","clientNo":"0123456789abcdef0123456789abcdef","clientType":412,"version":"1.1.30","versionCode":"100130"})
        );
        assert_eq!(requests.iter().filter(|r| r.command == 3).count(), 1);
        let auth = &requests[1].body;
        assert_eq!(
            auth["loginType"],
            if kind == CredentialKind::Email {
                "ursMailToken"
            } else {
                "ursPhoneTokenNew"
            }
        );
        assert_eq!(
            auth["urs"],
            if kind == CredentialKind::Email {
                "selected@synthetic.invalid"
            } else {
                "selected@synthetic.invalid@mobile.163.com"
            }
        );
        assert_eq!(auth["ccToken"], "synthetic-original-token");
        assert_eq!(auth["mobileId"], "synthetic-appid");
        assert_eq!(auth["appVer"], "1.1.30");
        assert_eq!(auth["osName"], "windows");
        let session = auth["session"]
            .as_str()
            .unwrap()
            .strip_suffix(":0")
            .unwrap();
        // Original token_urlsafe(18): 18 random bytes, 24 unpadded URL-safe chars.
        assert_eq!(session.len(), 24);
        assert!(
            session
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
        assert_eq!(
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(session)
                .unwrap()
                .len(),
            18
        );
        assert!(requests.iter().any(|r| r.command == 4));
    }
}
#[test]
fn a_response_must_match_service_command_and_original_sequence_before_its_identity_is_adopted() {
    let peer = Peer::new(Mode::WrongCorrelation);
    let control = OperationControl::default();
    let mut session = start(
        &peer,
        CredentialKind::Email,
        &control,
        Duration::from_secs(30),
    )
    .unwrap();
    assert_eq!(session.author_cookie(), "synthetic-cookie");
    assert_eq!(
        session.token_for(&control).unwrap().value,
        "synthetic-cc-token"
    );
    session.close().unwrap();
    let requests = peer.finish();
    assert_eq!(
        requests.iter().map(|r| r.command).collect::<Vec<_>>(),
        [1, 3, 8]
    );
}
#[test]
fn login_rejection_or_lost_reply_never_reconnects_replays_or_leaks_vendor_reasons() {
    for mode in [Mode::RejectHandshake, Mode::RejectLogin, Mode::DropLogin] {
        let peer = Peer::new(mode);
        let control = OperationControl::default();
        let error = start(
            &peer,
            CredentialKind::Email,
            &control,
            Duration::from_secs(30),
        )
        .err()
        .unwrap();
        let public = serde_json::to_string(&error).unwrap();
        assert!(!public.contains("synthetic-cookie"));
        assert!(!public.contains("private-token"));
        let requests = peer.finish();
        assert_eq!(requests.iter().filter(|r| r.command == 1).count(), 1);
        assert_eq!(
            requests.iter().filter(|r| r.command == 3).count(),
            if mode == Mode::RejectHandshake { 0 } else { 1 }
        );
    }
}
#[test]
fn jwt_exchange_uses_the_cc_login_token_and_only_one_explicit_server_timestamp_correction() {
    let peer = Peer::new(Mode::Happy);
    let control = OperationControl::default();
    let mut session = start(
        &peer,
        CredentialKind::Email,
        &control,
        Duration::from_secs(30),
    )
    .unwrap();
    let server = Server::new(vec![
        Reply::Json(json!({"code":"INVALID_TIMESTAMP","time":1793500200u64})),
        Reply::Json(json!({"code":"OK","data":{"jwt":"synthetic-jwt"}})),
    ]);
    let http = Http::loopback(&control, server.url.clone());
    assert_eq!(session.jwt_with(&http, &control).unwrap(), "synthetic-jwt");
    session.close().unwrap();
    let requests = server.finish();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        let target = request.line.split_whitespace().nth(1).unwrap();
        let url = reqwest::Url::parse(&format!("https://api.cc.163.com{target}")).unwrap();
        let query = url
            .query_pairs()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(url.path(), "/v1/jwt");
        assert_eq!(query.get("appid").unwrap(), "webcc");
        assert_eq!(query.get("uid").unwrap(), "9007199254740993");
        assert_eq!(query.get("eid").unwrap(), "0");
        let expected = format!(
            "{:x}",
            Md5::digest(
                format!(
                    "webcc_{}_synthetic-cc-token",
                    query.get("timestamp").unwrap()
                )
                .as_bytes()
            )
        );
        assert!(
            request
                .headers
                .to_ascii_lowercase()
                .contains(&format!("authentication: {expected}"))
        );
        assert!(!request.headers.contains("synthetic-cc-token"));
        assert!(!request.headers.to_ascii_lowercase().contains("cookie:"));
    }
    assert!(requests[1].line.contains("timestamp=1793500200"));
    let ws = peer.finish();
    assert_eq!(ws.iter().map(|r| r.command).collect::<Vec<_>>(), [1, 3, 8]);
}
#[test]
fn an_idle_kickout_retires_the_task_capability_without_rc_token_or_new_login() {
    let peer = Peer::new(Mode::Kick);
    let control = OperationControl::default();
    let mut session = start(
        &peer,
        CredentialKind::Email,
        &control,
        Duration::from_secs(30),
    )
    .unwrap();
    thread::sleep(Duration::from_millis(80));
    assert_eq!(session.token_for(&control).err().unwrap().exit_code, 3);
    let error = session.close().unwrap_err();
    assert_eq!(error.exit_code, 3);
    assert!(!error.to_string().contains("private-kick-reason"));
    assert_eq!(peer.finish().iter().filter(|r| r.command == 3).count(), 1);
}
#[test]
fn closing_an_inflight_heartbeat_or_cancelled_owner_joins_the_socket_loop() {
    for mode in [Mode::Happy, Mode::HoldHeartbeat] {
        let peer = Peer::new(mode);
        let control = OperationControl::default();
        let mut session = start(
            &peer,
            CredentialKind::Email,
            &control,
            Duration::from_millis(10),
        )
        .unwrap();
        thread::sleep(Duration::from_millis(70));
        if mode == Mode::Happy {
            control.tracked_cancel_handle().unwrap().cancel();
            assert_eq!(session.token_for(&control).err().unwrap().exit_code, 7);
        }
        let started = Instant::now();
        session.close().unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(peer.finish().iter().filter(|r| r.command == 3).count(), 1);
    }
}
#[test]
fn protocol_packet_lengths_json_ambiguity_and_compression_fail_at_the_native_boundary() {
    let control = OperationControl::default();
    let bytes = response(
        protocol::SID,
        8,
        "seq",
        0,
        &json!({"token":"synthetic","nowTime":1}),
    );
    let parsed = protocol::Response::decode(&bytes, &control).unwrap();
    assert_eq!(parsed.sequence.as_deref(), Some("seq"));
    let mut encoded = ZlibEncoder::new(Vec::new(), Compression::default());
    encoded.write_all(&bytes[16..]).unwrap();
    let zipped = encoded.finish().unwrap();
    let mut compressed = bytes[..16].to_vec();
    compressed[12..16].copy_from_slice(&0x20000u32.to_le_bytes());
    compressed.extend_from_slice(&[0; 4]);
    compressed.extend_from_slice(&zipped);
    assert_eq!(
        protocol::Response::decode(&compressed, &control)
            .unwrap()
            .data["token"],
        "synthetic"
    );
    for truncated in [
        &bytes[..15],
        &bytes[..bytes.len() - 1],
        &compressed[..compressed.len() - 2],
    ] {
        assert!(protocol::Response::decode(truncated, &control).is_err());
    }
    let bad = response(protocol::SID, 8, "seq", 0, &json!({}));
    let mut invalid = bad[..16].to_vec();
    invalid.extend_from_slice(&[2, 0, 255, 255]);
    assert!(protocol::Response::decode(&invalid, &control).is_err());
    let prop = br#"{"extInfo":"{}","extInfo":"{}"}"#;
    let mut duplicate = bad[..16].to_vec();
    duplicate.extend_from_slice(&(prop.len() as u16).to_le_bytes());
    duplicate.extend_from_slice(prop);
    duplicate.extend_from_slice(&0u32.to_le_bytes());
    duplicate.extend_from_slice(&0u16.to_le_bytes());
    duplicate.extend_from_slice(&2u16.to_le_bytes());
    duplicate.extend_from_slice(b"{}");
    assert!(protocol::Response::decode(&duplicate, &control).is_err());
    let too_large = json!({"payload":"x".repeat(65536)});
    assert!(protocol::encode(3, "seq", &too_large).is_err());
}

struct TaskSigner {
    closed: Arc<AtomicBool>,
    fail_close: bool,
}

#[test]
fn native_task_rebinds_author_deadlines_after_startup_and_cancelled_rpc_without_reauth() {
    let peer = Peer::new(Mode::Happy);
    let startup = OperationControl::uncancelable(Instant::now() + Duration::from_millis(500));
    let cc = start(
        &peer,
        CredentialKind::Email,
        &startup,
        Duration::from_millis(20),
    )
    .unwrap();
    let author = Server::new(vec![
        task_clock(),
        task_login(),
        Reply::Json(json!({"code":0,"result":{"selected":"one"}})),
        Reply::Json(json!({"code":0,"result":{"selected":"two"}})),
    ]);
    let closed = Arc::new(AtomicBool::new(false));
    let mut session = crate::dd::session::NativeSession::attach(cc, CredentialKind::Email, |cc| {
        Ok(task_author(cc, &author, &startup, closed.clone(), false))
    })
    .unwrap();
    thread::sleep(Duration::from_millis(550));
    assert!(startup.check().is_err());
    let first = OperationControl::default();
    session.begin_operation(&first).unwrap();
    assert_eq!(
        session
            .get("/addon/detail", &[("sn", "one".into())])
            .unwrap()["result"]["selected"],
        "one"
    );
    first.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(session.get("/addon/detail", &[]).unwrap_err().exit_code, 7);
    session
        .begin_operation(&OperationControl::default())
        .unwrap();
    assert_eq!(
        session
            .get("/addon/detail", &[("sn", "two".into())])
            .unwrap()["result"]["selected"],
        "two"
    );
    session.close().unwrap();
    assert!(closed.load(Ordering::Acquire));
    let requests = author.finish();
    assert_eq!(requests.len(), 4);
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.contains("/login/dflogin"))
            .count(),
        1
    );
    assert_eq!(peer.finish().iter().filter(|r| r.command == 3).count(), 1);
}

#[test]
fn owned_cc_connection_outlives_startup_but_each_token_rpc_keeps_its_own_deadline() {
    for mode in [Mode::Happy, Mode::HoldToken] {
        let peer = Peer::new(mode);
        let startup = OperationControl::uncancelable(Instant::now() + Duration::from_millis(500));
        let mut session = CcSession::start_at(
            credential(CredentialKind::Email),
            "synthetic-appid".into(),
            device(),
            &startup,
            peer.url.clone(),
            Duration::from_millis(20),
        )
        .unwrap();
        thread::sleep(Duration::from_millis(550));
        assert!(startup.check().is_err());
        if mode == Mode::Happy {
            assert_eq!(
                session
                    .token_for(&OperationControl::default())
                    .unwrap()
                    .value,
                "synthetic-cc-token"
            );
            session.close().unwrap();
        } else {
            let rpc = OperationControl::uncancelable(Instant::now() + Duration::from_millis(80));
            assert_eq!(session.token_for(&rpc).err().unwrap().exit_code, 5);
            assert_eq!(session.close().unwrap_err().exit_code, 5);
        }
        let requests = peer.finish();
        assert_eq!(requests.iter().filter(|r| r.command == 3).count(), 1);
        assert_eq!(requests.iter().filter(|r| r.command == 8).count(), 1);
        assert!(requests.iter().any(|r| r.command == 4));
    }
}

#[test]
fn native_task_with_a_lost_cc_link_refuses_another_author_request_and_still_releases_the_signer() {
    let peer = Peer::new(Mode::KickOnHeartbeat);
    let control = OperationControl::default();
    let cc = start(
        &peer,
        CredentialKind::Email,
        &control,
        Duration::from_millis(100),
    )
    .unwrap();
    let author = Server::new(vec![task_clock(), task_login()]);
    let closed = Arc::new(AtomicBool::new(false));
    let mut session = crate::dd::session::NativeSession::attach(cc, CredentialKind::Email, |cc| {
        Ok(task_author(cc, &author, &control, closed.clone(), false))
    })
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while session.identity()["cc"]["active"] != false {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(session.get("/addon/detail", &[]).unwrap_err().exit_code, 3);
    assert_eq!(session.close().unwrap_err().exit_code, 3);
    assert!(closed.load(Ordering::Acquire));
    assert_eq!(author.finish().len(), 2);
    assert_eq!(peer.finish().iter().filter(|r| r.command == 3).count(), 1);
}
impl crate::dd::Signer for TaskSigner {
    fn sign(&mut self, url: &str, _: Option<&[u8]>, _: &[(&str, &str)]) -> Result<String> {
        let mut url = reqwest::Url::parse(url).unwrap();
        url.query_pairs_mut()
            .append_pair("nep_signature", "synthetic-task-signature");
        Ok(url.into())
    }
    fn close(self: Box<Self>) -> Result<()> {
        self.closed.store(true, Ordering::Release);
        if self.fail_close {
            Err(ToolkitError::new(
                "synthetic.signer_close",
                "synthetic SDK close failure",
                3,
            ))
        } else {
            Ok(())
        }
    }
}
fn task_author(
    cc: &CcSession,
    server: &Server,
    control: &OperationControl,
    closed: Arc<AtomicBool>,
    fail_close: bool,
) -> crate::dd::DdTransport {
    crate::dd::DdTransport {
        http: Http::loopback(control, server.url.clone()),
        control: control.clone(),
        signer: Some(Box::new(TaskSigner { closed, fail_close })),
        cookie: cc.author_cookie().to_owned(),
        token: String::new(),
        clock: None,
    }
}
fn task_clock() -> Reply {
    Reply::Json(json!({"code":200,"result":{"ts":1793500000u64}}))
}
fn task_login() -> Reply {
    Reply::Json(
        json!({"code":200,"result":{"uid":9007199254740993u64,"token":"synthetic-author-token","nickname":"作者"}}),
    )
}
#[test]
fn native_task_composes_one_cc_jwt_and_author_login_then_reuses_them_for_serial_business() {
    let peer = Peer::new(Mode::Happy);
    let control = OperationControl::default();
    let cc = start(
        &peer,
        CredentialKind::Mobile,
        &control,
        Duration::from_millis(20),
    )
    .unwrap();
    let jwt = Server::new(vec![Reply::Json(
        json!({"code":"OK","data":{"jwt":"synthetic-startup-jwt"}}),
    )]);
    let author = Server::new(vec![
        task_clock(),
        task_login(),
        Reply::Json(json!({"code":0,"result":{"sn":"opaque-author-resource"}})),
        Reply::Json(json!({"code":0,"result":{"saved":true}})),
    ]);
    let closed = Arc::new(AtomicBool::new(false));
    let mut session = crate::dd::session::NativeSession::attach(cc, CredentialKind::Mobile, |cc| {
        assert_eq!(
            cc.jwt_with(&Http::loopback(&control, jwt.url.clone()), &control)
                .unwrap(),
            "synthetic-startup-jwt"
        );
        Ok(task_author(cc, &author, &control, closed.clone(), false))
    })
    .unwrap();
    let state = session.identity();
    assert_eq!(state["author"]["author_id"], "9007199254740993");
    assert_eq!(state["session_source"], "native-login");
    assert_eq!(state["native_login_count"], 1);
    for secret in [
        "synthetic-startup-jwt",
        "synthetic-original-token",
        "synthetic-author-token",
        "synthetic-cookie",
        "selected@synthetic.invalid",
        "clientNo",
        "00:11:22:33:44:55",
    ] {
        assert!(!state.to_string().contains(secret));
    }
    assert_eq!(
        session
            .get("/addon/detail", &[("sn", "opaque-author-resource".into())])
            .unwrap()["result"]["sn"],
        "opaque-author-resource"
    );
    session
        .post(
            "/addon/update",
            &[],
            &json!({"name":"中文","clear":null}),
            true,
        )
        .unwrap();
    thread::sleep(Duration::from_millis(60));
    session.close().unwrap();
    assert!(closed.load(Ordering::Acquire));
    assert_eq!(jwt.finish().len(), 1);
    let requests = author.finish();
    assert_eq!(requests.len(), 4);
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.contains("/login/dflogin"))
            .count(),
        1
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[3].body).unwrap(),
        json!({"name":"中文","clear":null})
    );
    let cc_requests = peer.finish();
    assert_eq!(cc_requests.iter().filter(|r| r.command == 3).count(), 1);
    assert_eq!(cc_requests.iter().filter(|r| r.command == 8).count(), 1);
    assert!(cc_requests.iter().any(|r| r.command == 4));
}
#[test]
fn native_task_startup_rejection_releases_author_and_cc_even_when_sdk_close_fails() {
    let peer = Peer::new(Mode::Happy);
    let control = OperationControl::default();
    let cc = start(
        &peer,
        CredentialKind::Email,
        &control,
        Duration::from_secs(30),
    )
    .unwrap();
    let author = Server::new(vec![
        task_clock(),
        Reply::Json(json!({"code":403,"msg":"private-sdk-token synthetic-cookie"})),
    ]);
    let closed = Arc::new(AtomicBool::new(false));
    let error = crate::dd::session::NativeSession::attach(cc, CredentialKind::Email, |cc| {
        Ok(task_author(cc, &author, &control, closed.clone(), true))
    })
    .err()
    .unwrap();
    assert_eq!(error.exit_code, 5);
    assert!(closed.load(Ordering::Acquire));
    assert_eq!(
        error.details["cleanup_error"],
        "synthetic SDK close failure"
    );
    assert!(!error.to_string().contains("private-sdk-token"));
    assert_eq!(author.finish().len(), 2);
    assert_eq!(peer.finish().iter().filter(|r| r.command == 3).count(), 1);
}
#[test]
fn native_task_uncertain_write_does_not_repeat_login_or_post_and_still_releases_its_link() {
    let peer = Peer::new(Mode::Happy);
    let control = OperationControl::default();
    let cc = start(
        &peer,
        CredentialKind::Email,
        &control,
        Duration::from_secs(30),
    )
    .unwrap();
    let author = Server::new(vec![
        task_clock(),
        task_login(),
        Reply::Drop,
        Reply::Json(json!({"code":0,"result":{"readback":true}})),
    ]);
    let closed = Arc::new(AtomicBool::new(false));
    let mut session = crate::dd::session::NativeSession::attach(cc, CredentialKind::Email, |cc| {
        Ok(task_author(cc, &author, &control, closed.clone(), false))
    })
    .unwrap();
    assert_eq!(
        session
            .post("/addon/update", &[], &json!({"sn":"original"}), true)
            .unwrap_err()
            .exit_code,
        5
    );
    assert_eq!(
        session
            .get("/addon/detail", &[("sn", "original".into())])
            .unwrap()["result"]["readback"],
        true
    );
    session.close().unwrap();
    assert!(closed.load(Ordering::Acquire));
    let requests = author.finish();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("POST "))
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.contains("/login/dflogin"))
            .count(),
        1
    );
    assert_eq!(peer.finish().iter().filter(|r| r.command == 3).count(), 1);
}
