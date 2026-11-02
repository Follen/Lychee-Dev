//! One native CC login and one owned WebSocket loop. HTTP uploads cannot block
//! its heartbeat. A lost connection ends this capability; it never reconnects,
//! repeats login, uses an rcToken or changes the desktop application's session.
use super::{SavedCredential, USER_AGENT_VALUE};
use crate::transport::{Http, Origin, secret_header};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use md5::{Digest, Md5};
use reqwest::header::{HeaderValue, USER_AGENT};
use serde_json::{Value, json};
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async_tls_with_config,
    tungstenite::{Message, client::IntoClientRequest, protocol::WebSocketConfig},
};
mod lifetime;
mod protocol;
use lifetime::Lifetime;
const ENDPOINT: &str = "wss://teamlink-ws.cc.163.com/";
type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
pub struct Device {
    pub client_no: String,
    pub mac: String,
    pub os_version: String,
    pub bi_desc: Value,
}
struct Identity {
    cookie: String,
    uid: Value,
    eid: Value,
}
struct Token {
    value: String,
    server_ms: u64,
    sampled: Instant,
}
enum Command {
    Token(SyncSender<Result<Token>>, OperationControl),
    Stop,
}
pub struct CcSession {
    sender: SyncSender<Command>,
    worker: Option<JoinHandle<Result<()>>>,
    control: Lifetime,
    identity: Identity,
    active: Arc<AtomicBool>,
}
impl CcSession {
    pub fn start(
        credential: SavedCredential,
        appid: String,
        device: Device,
        control: &OperationControl,
    ) -> Result<Self> {
        Self::start_at(
            credential,
            appid,
            device,
            control,
            ENDPOINT.into(),
            Duration::from_secs(30),
        )
    }
    fn start_at(
        credential: SavedCredential,
        appid: String,
        device: Device,
        control: &OperationControl,
        endpoint: String,
        heartbeat: Duration,
    ) -> Result<Self> {
        let lifetime = Lifetime::new();
        control.check()?;
        if appid.trim().is_empty()
            || appid.len() > 4096
            || appid.contains('\0')
            || device.client_no.len() != 32
            || !device.client_no.bytes().all(|b| b.is_ascii_hexdigit())
            || device.mac.is_empty()
            || device.mac.len() > 128
            || device.os_version.len() > 256
        {
            return Err(ToolkitError::new(
                "publish.dd_cc_device",
                "CC application/device identity is missing or invalid",
                3,
            ));
        }
        #[cfg(not(test))]
        if endpoint != ENDPOINT {
            return Err(ToolkitError::new(
                "publish.dd_cc_origin",
                "CC requires its fixed secure WebSocket service",
                3,
            ));
        }
        #[cfg(test)]
        if endpoint != ENDPOINT {
            let url = reqwest::Url::parse(&endpoint)
                .map_err(|_| protocol::invalid("invalid test endpoint"))?;
            assert_eq!(url.scheme(), "ws");
            assert_eq!(url.host_str(), Some("127.0.0.1"));
        }
        let control = control.ordered_child(control.deadline())?;
        let worker_control = control.clone();
        let worker_lifetime = lifetime.clone();
        let active = Arc::new(AtomicBool::new(false));
        let worker_active = active.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let (ready_send, ready) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("dd-cc-session".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|_| {
                        ToolkitError::new(
                            "publish.dd_cc_runtime",
                            "CC session runtime is unavailable",
                            3,
                        )
                    });
                let result = match runtime {
                    Err(error) => {
                        let _ = ready_send.send(Err(error.clone()));
                        Err(error)
                    }
                    Ok(runtime) => runtime.block_on(run(
                        credential,
                        appid,
                        device,
                        endpoint,
                        heartbeat,
                        receiver,
                        ready_send,
                        &worker_control,
                        &worker_lifetime,
                        &worker_active,
                    )),
                };
                worker_active.store(false, Ordering::Release);
                result
            })
            .map_err(|_| {
                ToolkitError::new(
                    "publish.dd_cc_thread",
                    "CC session owner could not be started",
                    3,
                )
            })?;
        let identity = receive(&ready, &control);
        match identity {
            Ok(identity) => Ok(Self {
                sender,
                worker: Some(worker),
                control: lifetime,
                identity,
                active,
            }),
            Err(error) => {
                control.tracked_cancel_handle()?.cancel();
                lifetime.stop();
                drop(sender);
                let _ = worker.join();
                Err(error)
            }
        }
    }
    pub fn identity(&self) -> Value {
        json!({"cc_uid":self.identity.uid,"cc_eid":self.identity.eid,"native_login_count":1,"active":self.active.load(Ordering::Acquire)&&self.worker.as_ref().is_some_and(|worker|!worker.is_finished())})
    }
    pub fn author_cookie(&self) -> &str {
        &self.identity.cookie
    }
    pub(super) fn check_active(&self) -> Result<()> {
        self.control.check()?;
        if !self.active.load(Ordering::Acquire)
            || !self
                .worker
                .as_ref()
                .is_some_and(|worker| !worker.is_finished())
        {
            return Err(lost());
        }
        Ok(())
    }
    fn token_for(&mut self, control: &OperationControl) -> Result<Token> {
        control.check()?;
        self.check_active()?;
        let (sender, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(Command::Token(sender, control.clone()))
            .map_err(|_| lost())?;
        receive(&receiver, control)
    }
    pub(super) fn jwt_for(&mut self, control: &OperationControl) -> Result<String> {
        let http = Http::new(control)?;
        self.jwt_with(&http, control)
    }
    pub(super) fn jwt_with(&mut self, http: &Http, control: &OperationControl) -> Result<String> {
        let token = self.token_for(control)?;
        let mut timestamp = token.server_ms / 1000 + token.sampled.elapsed().as_secs();
        for attempt in 0..2 {
            control.check()?;
            self.control.check()?;
            let sign = format!(
                "{:x}",
                Md5::digest(format!("webcc_{timestamp}_{}", token.value).as_bytes())
            );
            let mut headers = secret_header("authentication", &sign)?;
            headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
            let query = [
                ("appid", "webcc".into()),
                ("uid", identity_text(&self.identity.uid)?),
                ("eid", identity_text(&self.identity.eid)?),
                ("timestamp", timestamp.to_string()),
            ];
            let value = http.get(Origin::DdCcApi, "/v1/jwt", &query, headers)?;
            if value.get("code").and_then(Value::as_str) == Some("OK") {
                return value
                    .pointer("/data/jwt")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty() && s.len() <= 65536)
                    .map(str::to_owned)
                    .ok_or_else(|| protocol::invalid("CC JWT response omitted its token"));
            }
            if attempt == 0
                && value.get("code").and_then(Value::as_str) == Some("INVALID_TIMESTAMP")
            {
                timestamp = value
                    .get("time")
                    .and_then(Value::as_u64)
                    .filter(|time| *time > 0)
                    .ok_or_else(|| protocol::invalid("CC JWT time correction is invalid"))?;
                continue;
            }
            return Err(ToolkitError::new(
                "publish.dd_jwt_rejected",
                "CC JWT exchange was rejected",
                5,
            ));
        }
        unreachable!("two bounded JWT attempts return")
    }
    pub fn close(mut self) -> Result<()> {
        self.finish()
    }
    fn finish(&mut self) -> Result<()> {
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        let _ = self.sender.try_send(Command::Stop);
        // Disconnection/cancellation wakes an in-progress network future. The
        // loop still performs its bounded transport cleanup before joining.
        self.control.stop();
        worker
            .join()
            .map_err(|_| ToolkitError::new("publish.dd_cc_owner", "CC session owner panicked", 8))?
    }
}
impl Drop for CcSession {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
fn receive<T>(receiver: &Receiver<Result<T>>, control: &OperationControl) -> Result<T> {
    loop {
        control.check()?;
        match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(value) => return value,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(lost()),
        }
    }
}
fn identity_text(value: &Value) -> Result<String> {
    match value {
        Value::String(s) if !s.is_empty() => Ok(s.clone()),
        Value::Number(n) if n.as_u64().is_some() => Ok(n.to_string()),
        _ => Err(protocol::invalid("CC login omitted uid or eid")),
    }
}
async fn controlled<T>(
    control: &OperationControl,
    lifetime: Option<&Lifetime>,
    future: impl Future<Output = Result<T>>,
) -> Result<T> {
    control.check()?;
    if let Some(lifetime) = lifetime {
        lifetime.check()?;
    }
    tokio::pin!(future);
    let mut tick = tokio::time::interval(Duration::from_millis(20));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {result=&mut future=>return result,_=tick.tick()=>{control.check()?;if let Some(lifetime)=lifetime{lifetime.check()?;}},}
    }
}
async fn packet(
    socket: &mut Socket,
    control: &OperationControl,
    lifetime: Option<&Lifetime>,
) -> Result<protocol::Response> {
    loop {
        let message = controlled(control, lifetime, async {
            socket.next().await.ok_or_else(lost)?.map_err(|_| lost())
        })
        .await?;
        match message {
            Message::Binary(bytes) => return protocol::Response::decode(&bytes, control),
            Message::Ping(_) => {
                controlled(control, lifetime, async {
                    socket.flush().await.map_err(|_| lost())
                })
                .await?;
            }
            Message::Pong(_) => {}
            Message::Close(_) => return Err(lost()),
            _ => {
                return Err(protocol::invalid(
                    "CC session received a non-binary application message",
                ));
            }
        }
    }
}
async fn request(
    socket: &mut Socket,
    cid: u16,
    sequence: &str,
    body: &Value,
    control: &OperationControl,
    lifetime: Option<&Lifetime>,
) -> Result<Value> {
    let request_control = control
        .ordered_child((Instant::now() + Duration::from_secs(15)).min(control.deadline()))?;
    let control = &request_control;
    let bytes = protocol::encode(cid, sequence, body)?;
    controlled(control, lifetime, async {
        socket
            .send(Message::Binary(bytes.into()))
            .await
            .map_err(|_| lost())
    })
    .await?;
    loop {
        let response = packet(socket, control, lifetime).await?;
        if response.sid == protocol::SID && response.command == 5 {
            return Err(ToolkitError::new(
                "publish.dd_cc_kicked",
                "CC task session was retired by the server",
                3,
            ));
        }
        if response.sid != protocol::SID
            || response.command != cid
            || response.sequence.as_deref() != Some(sequence)
        {
            continue;
        }
        if response.code != 0 {
            return Err(ToolkitError::new(
                "publish.dd_cc_rejected",
                "CC session request was rejected",
                5,
            )
            .with_details(json!({"command":cid,"business_code":response.code})));
        }
        return Ok(response.data);
    }
}
async fn run(
    credential: SavedCredential,
    appid: String,
    device: Device,
    endpoint: String,
    heartbeat: Duration,
    receiver: Receiver<Command>,
    ready: SyncSender<Result<Identity>>,
    control: &OperationControl,
    lifetime: &Lifetime,
    active: &AtomicBool,
) -> Result<()> {
    let opening = control
        .ordered_child((Instant::now() + Duration::from_secs(45)).min(control.deadline()))?;
    let handshake_request = endpoint
        .into_client_request()
        .map_err(|_| protocol::invalid("CC endpoint request is invalid"))?;
    let config = WebSocketConfig::default()
        .max_message_size(Some(protocol::LIMIT))
        .max_frame_size(Some(protocol::LIMIT));
    let connected = controlled(&opening, None, async {
        connect_async_tls_with_config(handshake_request, Some(config), true, None)
            .await
            .map_err(|_| lost())
    })
    .await;
    let (mut socket, _) = match connected {
        Ok(value) => value,
        Err(error) => {
            let _ = ready.send(Err(error.clone()));
            return Err(error);
        }
    };
    let session = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        lycheedev_windows::random::bytes::<18>()
            .map_err(|_| protocol::invalid("CC task identity could not be generated"))?,
    );
    let mut sequence = 0u64;
    let next = |sequence: &mut u64| {
        let value = format!("{session}{sequence}");
        *sequence += 1;
        value
    };
    let login=async{
        let handshake=json!({"mac":device.mac,"clientNo":device.client_no,"clientType":412,"version":"1.1.30","versionCode":"100130"});
        request(&mut socket,1,&next(&mut sequence),&handshake,&opening,None).await?;
        let auth=json!({"loginType":credential.login_type(),"ccToken":credential.token,"mobileId":appid,
            "session":format!("{session}:0"),"urs":credential.cc_username(),"appVer":"1.1.30","osName":"windows","osVer":device.os_version,"macAddr":device.mac,"biDesc":device.bi_desc});
        let info=request(&mut socket,3,&next(&mut sequence),&auth,&opening,None).await?;
        let cookie=info.get("cookieValue").and_then(Value::as_str).filter(|v|!v.is_empty()&&!v.contains([';','\r','\n','\0'])&&v.len()<=65536)
            .ok_or_else(||protocol::invalid("CC login omitted a valid author cookie"))?;
        if info.get("cookieName").and_then(Value::as_str)!=Some("NTES_YD_SESS"){return Err(protocol::invalid("CC login returned a different cookie capability"));}
        let uid=info.get("uid").cloned().unwrap_or(Value::Null);let eid=info.get("eid").cloned().unwrap_or(Value::Null);
        identity_text(&uid)?;identity_text(&eid)?;
        Ok(Identity{cookie:cookie.into(),uid,eid})
    }.await;
    if let Err(error) = login {
        let _ = ready.send(Err(error.clone()));
        drop(socket);
        return Err(error);
    }
    active.store(true, Ordering::Release);
    if ready.send(login).is_err() {
        drop(socket);
        return Ok(());
    }
    let mut due = Instant::now() + heartbeat;
    let outcome = loop {
        if lifetime.is_cancelled() {
            break Ok(());
        }
        if let Err(error) = lifetime.check() {
            break Err(error);
        }
        match receiver.try_recv() {
            Ok(Command::Stop) | Err(TryRecvError::Disconnected) => break Ok(()),
            Ok(Command::Token(reply, operation)) => {
                let value = request(
                    &mut socket,
                    8,
                    &next(&mut sequence),
                    &json!({}),
                    &operation,
                    Some(lifetime),
                )
                .await
                .and_then(|data| {
                    let token = data
                        .get("token")
                        .and_then(Value::as_str)
                        .filter(|v| !v.trim().is_empty() && v.len() <= 65536)
                        .ok_or_else(|| {
                            protocol::invalid("CC login-token response omitted its token")
                        })?;
                    let server_ms = data
                        .get("nowTime")
                        .and_then(Value::as_u64)
                        .filter(|v| *v > 0)
                        .ok_or_else(|| {
                            protocol::invalid("CC login-token response omitted server time")
                        })?;
                    Ok(Token {
                        value: token.into(),
                        server_ms,
                        sampled: Instant::now(),
                    })
                });
                let failed = value.as_ref().err().cloned();
                let _ = reply.send(value);
                if let Some(error) = failed {
                    break Err(error);
                }
            }
            Err(TryRecvError::Empty) => {}
        }
        if Instant::now() >= due {
            let operation = lifetime.operation()?;
            if let Err(error) = request(
                &mut socket,
                4,
                &next(&mut sequence),
                &json!({}),
                &operation,
                Some(lifetime),
            )
            .await
            {
                break Err(error);
            }
            due = Instant::now() + heartbeat;
        }
        // Keep reading when no command is running so ping/close/kick-out is
        // observed independently of an author HTTP operation on the caller.
        let idle = tokio::time::sleep(Duration::from_millis(20));
        tokio::pin!(idle);
        tokio::select! {
            message=socket.next()=>match message{
                Some(Ok(Message::Ping(_)))=>{let operation=lifetime.operation()?;if let Err(error)=controlled(&operation,Some(lifetime),async{socket.flush().await.map_err(|_|lost())}).await{break Err(error);}},
                Some(Ok(Message::Pong(_)))=>{},
                Some(Ok(Message::Binary(bytes)))=>match protocol::Response::decode(&bytes,&lifetime.operation()?){
                    Ok(response)if response.sid==protocol::SID&&response.command==5=>break Err(ToolkitError::new("publish.dd_cc_kicked","CC task session was retired by the server",3)),
                    Ok(_)=>{},Err(error)=>break Err(error),
                },
                _=>break Err(lost()),
            },
            _=&mut idle=>{},
        }
    };
    // Retire the authentication capability before transport cleanup. A live
    // cleanup thread no longer authorizes another author request or JWT.
    active.store(false, Ordering::Release);
    let cleanup = OperationControl::uncancelable(Instant::now() + Duration::from_secs(5));
    let closed = controlled(&cleanup, None, async {
        socket.close(None).await.map_err(|_| lost())
    })
    .await;
    drop(socket);
    match closed {
        Err(error) => Err(error),
        Ok(()) => match outcome {
            Err(error) if error.code == "command.cancelled" => Ok(()),
            value => value,
        },
    }
}
fn lost() -> ToolkitError {
    ToolkitError::new(
        "publish.dd_cc_lost",
        "CC task connection ended; no reconnect or login replay was attempted",
        3,
    )
}
#[cfg(test)]
mod tests;
