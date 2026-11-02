use super::{
    protocol::{self, Api, Session, checked_result},
    *,
};
use crate::{contract, input, provider::Provider};
use base64::{Engine, engine::general_purpose::STANDARD};
use lycheedev_windows::credentials;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
};

const TTL: i64 = 300;
const ENTROPY: &[u8] = b"lycheedev.publish.blackbox.auth.v1";
const TERMINAL: &[&str] = &[
    "expired",
    "cancelled",
    "failed",
    "captcha_required",
    "device_verification_required",
    "verification_required",
];
#[derive(Clone, Copy)]
pub enum AuthAction {
    Start,
    Submit,
    Renew,
    Forget,
}
impl AuthAction {
    pub fn key(self) -> &'static str {
        match self {
            Self::Start => "login-start",
            Self::Submit => "login-submit",
            Self::Renew => "renew",
            Self::Forget => "forget",
        }
    }
}
pub enum AuthWrite {
    Start {
        method: String,
        phone: Option<String>,
    },
    Submit {
        id: String,
        code: String,
    },
    Renew,
    Forget,
}
impl AuthWrite {
    pub fn parse(action: AuthAction, bytes: &[u8]) -> Result<Self> {
        let value = input::document(bytes)?;
        contract::input_shape(Provider::Blackbox, "session", action.key())?.admit(&value)?;
        match action {
            AuthAction::Start => {
                let method = value["method"].as_str().unwrap();
                let phone = value.get("phone").and_then(Value::as_str);
                if method == "sms"
                    && !phone.is_some_and(|s| {
                        s.len() == 11
                            && s.as_bytes()[0] == b'1'
                            && (b'3'..=b'9').contains(&s.as_bytes()[1])
                            && s.bytes().all(|b| b.is_ascii_digit())
                    })
                {
                    return Err(input::invalid(
                        "$.phone",
                        "SMS phone must be an 11-digit mainland China mobile number",
                    ));
                }
                if method == "wechat" && phone.is_some() {
                    return Err(input::invalid(
                        "$.phone",
                        "WeChat login does not accept a phone field",
                    ));
                }
                Ok(Self::Start {
                    method: method.into(),
                    phone: phone.map(str::to_owned),
                })
            }
            AuthAction::Submit => {
                let id = value["login_id"].as_str().unwrap();
                validate_login_id(id)?;
                let code = value["code"].as_str().unwrap();
                if !(4..=8).contains(&code.len()) || !code.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(input::invalid(
                        "$.code",
                        "SMS code must contain 4..8 digits",
                    ));
                }
                Ok(Self::Submit {
                    id: id.into(),
                    code: code.into(),
                })
            }
            AuthAction::Renew => Ok(Self::Renew),
            AuthAction::Forget => Ok(Self::Forget),
        }
    }
    pub fn schema(action: AuthAction) -> Value {
        let mut schema = contract::input_shape(Provider::Blackbox, "session", action.key())
            .unwrap()
            .schema();
        schema.as_object_mut().unwrap().remove("x-validation-scope");
        if matches!(action, AuthAction::Start) {
            schema["oneOf"] = json!([
                {"properties":{"method":{"const":"sms"},"phone":{"type":"string","pattern":"^1[3-9][0-9]{9}$"}},"required":["phone"]},
                {"properties":{"method":{"const":"wechat"}},"not":{"required":["phone"]}}
            ]);
        }
        if matches!(action, AuthAction::Submit) {
            schema["properties"]["login_id"]["pattern"] = json!("^[a-f0-9]{32}$");
            schema["properties"]["code"]["pattern"] = json!("^[0-9]{4,8}$");
        }
        schema
    }
    pub fn plan(&self) -> Value {
        let (action, method) = match self {
            Self::Start { method, .. } => ("login-start", Some(method.as_str())),
            Self::Submit { .. } => ("login-submit", None),
            Self::Renew => ("renew", None),
            Self::Forget => ("forget", None),
        };
        json!({"platform":"blackbox","action":action,"method":method,"plan":true,"credential_reads":false,"remote_write":false,"local_write":false,"automatic_retry":false})
    }
    pub fn execute(self, home: &Path, control: &OperationControl) -> Result<Value> {
        Auth::open(home, control)?.finish(|auth| match self {
            Self::Start { method, phone } => auth.start(&method, phone.as_deref()),
            Self::Submit { id, code } => auth.submit(&id, &code),
            Self::Renew => auth
                .renew(true)
                .map(|session| authenticated(&session, None)),
            Self::Forget => auth.forget(),
        })
    }
}
pub(super) fn validate_login_id(id: &str) -> Result<()> {
    if id.len() != 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(input::invalid(
            "$.login_id",
            "login_id must be the original 32-character ID returned by login-start",
        ));
    }
    Ok(())
}
#[derive(Serialize, Deserialize)]
struct State {
    schema: String,
    active: Option<String>,
    sessions: BTreeMap<String, Session>,
    pending: Option<Pending>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            schema: "lycheedev.publish.blackbox.auth.v1".into(),
            active: None,
            sessions: BTreeMap::new(),
            pending: None,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Pending {
    id: String,
    created_at: i64,
    expires_at: i64,
    status: String,
    session: Session,
    phone_encrypted: Option<String>,
    candidate: Option<Session>,
    image_suffix: Option<String>,
    oauth_uuid: Option<String>,
    oauth_callback: Option<String>,
    oauth_state: Option<String>,
    oauth_next: Option<String>,
    oauth_last: Option<u16>,
    callback_attempted: bool,
}
pub(super) struct Auth {
    pub api: Api,
    root: PathBuf,
    state: State,
    lock: Option<storage::ExclusiveLock>,
    control: OperationControl,
}
impl Auth {
    pub fn open(home: &Path, control: &OperationControl) -> Result<Self> {
        let api = Api::new(control)?;
        let root = home.join("publish/blackbox/auth");
        let lock = storage::ExclusiveLock::resource_controlled(&root, "auth", control)?;
        let state_path = root.join("session.dpapi");
        storage::guard_path(&root, &state_path)?;
        let state = if state_path.try_exists()? {
            let (mut file, _) = storage::locked_sha256(&state_path, 4 * 1024 * 1024, control)?;
            let mut bytes = Vec::new();
            let result = file
                .seek(SeekFrom::Start(0))
                .and_then(|_| file.read_to_end(&mut bytes))
                .map_err(ToolkitError::from);
            storage::finish_file(file, result, control)?;
            let bytes =
                credentials::unprotect_current_user(&bytes, Some(ENTROPY)).map_err(|_| {
                    ToolkitError::new(
                        "publish.blackbox_storage",
                        "Heybox session cannot be decrypted by the current Windows user",
                        3,
                    )
                })?;
            let state: State = serde_json::from_value(input::value(&bytes)?)
                .map_err(|_| protocol_error("Heybox encrypted session is invalid"))?;
            if state.schema != "lycheedev.publish.blackbox.auth.v1"
                || state
                    .sessions
                    .keys()
                    .any(|s| !["sms", "wechat"].contains(&s.as_str()))
                || state
                    .active
                    .as_ref()
                    .is_some_and(|a| !state.sessions.contains_key(a))
            {
                return Err(protocol_error(
                    "Heybox encrypted session selection is invalid",
                ));
            }
            for (method, session) in &state.sessions {
                if &session.method != method {
                    return Err(protocol_error("Heybox login method changed"));
                }
                session.identity()?;
            }
            state
        } else {
            State::default()
        };
        Ok(Self {
            api,
            root,
            state,
            lock: Some(lock),
            control: control.clone(),
        })
    }
    pub fn finish(mut self, run: impl FnOnce(&mut Self) -> Result<Value>) -> Result<Value> {
        let result = run(&mut self);
        let close = self.lock.take().unwrap().close();
        match (result, close) {
            (value, Ok(())) => value,
            (Ok(value), Err(error)) => {
                Err(error.with_details(json!({"result":value,"cleanup_pending":true})))
            }
            (Err(mut error), Err(cleanup)) => {
                error.details["cleanup"] = json!({"code":cleanup.code,"message":cleanup.message});
                Err(error)
            }
        }
    }
    fn save(&self) -> Result<()> {
        // Persist observed login facts even if the request deadline just ended.
        let bytes = serde_json::to_vec(&self.state)?;
        let cipher = credentials::protect_current_user(&bytes, Some(ENTROPY)).map_err(|_| {
            ToolkitError::new(
                "publish.blackbox_storage",
                "cannot protect Heybox session for the current Windows user",
                3,
            )
        })?;
        storage::atomic_write(&self.root.join("session.dpapi"), &cipher)
    }
    fn save_pending(&mut self, pending: &Pending) -> Result<()> {
        self.state.pending = Some(pending.clone());
        self.save()
    }
    fn clear_pending(&mut self) -> Result<()> {
        if let Some(pending) = self.state.pending.as_ref() {
            validate_login_id(&pending.id)?;
            for suffix in ["png", "jpg"] {
                let path = self.root.join(format!("qr-{}.{}", pending.id, suffix));
                storage::guard_path(&self.root, &path)?;
                match fs::remove_file(path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        self.state.pending = None;
        self.save()
    }
    fn pending(&mut self, id: &str) -> Result<Option<Pending>> {
        let Some(pending) = self.state.pending.clone() else {
            return Ok(None);
        };
        validate_login_id(&pending.id)?;
        if pending.id != id {
            return Err(ToolkitError::new(
                "publish.blackbox_login_id",
                "login_id differs from the pending login",
                2,
            ));
        }
        if !["sms", "wechat"].contains(&pending.session.method.as_str())
            || pending.expires_at - pending.created_at != TTL
            || pending.created_at > now() + 5
        {
            return Err(protocol_error("Heybox pending login is invalid"));
        }
        if pending.expires_at <= now() {
            self.clear_pending()?;
            return Ok(None);
        }
        Ok(Some(pending))
    }
    fn public(&self, p: &Pending) -> Value {
        let mut result = json!({"platform":"blackbox","login_id":p.id,"method":p.session.method,"status":p.status,"authenticated":false,"expires_in":(p.expires_at-now()).max(0),"automatic_retry":false,"next":"publish blackbox session login-status --login-id <original-login-id>"});
        if !TERMINAL.contains(&p.status.as_str()) {
            if let Some(suffix) = &p.image_suffix {
                result["image_file"] = json!(self.root.join(format!("qr-{}.{}", p.id, suffix)));
            }
        }
        if p.status == "waiting_code" {
            result["next"] = json!(
                "publish blackbox session login-submit --input <original-login-id-and-code.json>"
            );
        }
        result
    }
    fn auth_request(
        &self,
        session: &mut Session,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
        mutation: bool,
    ) -> Result<Value> {
        self.api.request(
            session,
            path,
            query,
            body,
            mutation,
            device_risk_token().as_deref(),
        )
    }
    pub fn renew(&mut self, verify: bool) -> Result<Session> {
        let method = self.state.active.as_ref().ok_or_else(login_required)?;
        let mut session = self
            .state
            .sessions
            .get(method)
            .cloned()
            .ok_or_else(login_required)?;
        let expected = session.identity()?;
        let response =
            self.auth_request(&mut session, "/account/restore_login", &[], None, false)?;
        session.apply(&checked_result(&response)?, Some(&expected))?;
        if verify {
            self.verify(&mut session)?;
        }
        self.state
            .sessions
            .insert(session.method.clone(), session.clone());
        self.save()?;
        Ok(session)
    }
    pub fn doctor(&mut self) -> Result<Value> {
        if self.state.active.is_none() {
            return Ok(
                json!({"platform":"blackbox","status":"login_required","session_present":false,"authenticated":false,"api_ready":false,"next":"publish blackbox session login-start"}),
            );
        }
        self.renew(true).map(|s| authenticated(&s, None))
    }
    fn verify(&self, session: &mut Session) -> Result<()> {
        for (path, post, field) in [
            ("/wow/open_platform/module/list/", false, "moduleList"),
            (
                "/wow/get_user_configuration_backup_list/",
                true,
                "backupList",
            ),
        ] {
            let response = self.api.data(session, path, &[], post)?;
            if !response[field].is_array() {
                return Err(protocol_error(
                    "Heybox login was not verified by Workshop reads",
                ));
            }
        }
        Ok(())
    }
    fn start(&mut self, method: &str, phone: Option<&str>) -> Result<Value> {
        if let Some(p) = self.state.pending.clone() {
            if let Some(active) = self.pending(&p.id)? {
                if !TERMINAL.contains(&active.status.as_str()) {
                    return Err(ToolkitError::new(
                        "publish.blackbox_login_pending",
                        "continue the original Heybox login before starting another",
                        6,
                    )
                    .with_details(self.public(&active)));
                }
            }
            self.clear_pending()?;
        }
        let created_at = now();
        let mut p = Pending {
            id: random()?,
            created_at,
            expires_at: created_at + TTL,
            status: "starting".into(),
            session: Session {
                method: method.into(),
                device_id: random()?,
                ..Default::default()
            },
            phone_encrypted: None,
            candidate: None,
            image_suffix: None,
            oauth_uuid: None,
            oauth_callback: None,
            oauth_state: None,
            oauth_next: None,
            oauth_last: None,
            callback_attempted: false,
        };
        // Reserve the login before either an SMS or QR can be created.
        self.save_pending(&p)?;
        let result = (|| {
            let response =
                self.api
                    .fetch(&mut p.session, Url::parse(protocol::LOGIN).unwrap(), false)?;
            if response.status != 200 {
                return Err(protocol_error("Heybox login page did not return HTTP 200"));
            }
            p.session.web_version = web_version(&response.body)?;
            if method == "wechat" {
                return self.wechat_start(&mut p);
            }
            let modulus = hex::decode("d98235700222938d9e799c6fa65baaf74d2538102bcd62a0cc212fdb8428f641a1f9188fca65bd93069f8206f523f522c59e6250d24cc7cc7a0664243cf157af09f7c5ec978f185f5c98a42aa2d95f4ca6d5e28c55e622f3ec9486e9bfd583b017b6b4e894db56b1af07881d2d23460295869094e5e6e14c4b792c50c121444f").unwrap();
            let cipher = credentials::encrypt_rsa_pkcs1(
                &modulus,
                &[1, 0, 1],
                format!("+86{}", phone.unwrap()).as_bytes(),
            )
            .map_err(|_| {
                ToolkitError::new(
                    "publish.blackbox_phone_encryption",
                    "Windows could not encrypt the SMS phone number",
                    3,
                )
            })?;
            p.phone_encrypted = Some(STANDARD.encode(cipher));
            p.status = "sms_sending".into();
            self.save_pending(&p)?;
            let sent = self.auth_request(
                &mut p.session,
                "/account/get_login_code/",
                &[],
                Some(&json!({"phone_num":p.phone_encrypted})),
                true,
            )?;
            p.status = if sent["status"] == "ok" {
                "waiting_code".into()
            } else {
                failure(&sent).into()
            };
            self.save_pending(&p)?;
            let mut result = self.public(&p);
            result["sms_sent"] = json!(sent["status"] == "ok");
            Ok(result)
        })();
        match result {
            Ok(v) => Ok(v),
            Err(error) => {
                p.status = if p.status == "sms_sending" {
                    "sms_send_uncertain"
                } else {
                    "starting_uncertain"
                }
                .into();
                self.save_pending(&p)?;
                Err(auth_error(error, &p.id, "login-start"))
            }
        }
    }
    fn submit(&mut self, id: &str, code: &str) -> Result<Value> {
        let Some(mut p) = self.pending(id)? else {
            return Ok(expired(id));
        };
        if p.session.method != "sms" || p.status != "waiting_code" {
            return Err(ToolkitError::new(
                "publish.blackbox_login_state",
                "this original login is not awaiting an SMS code",
                2,
            ));
        }
        p.status = "verification_required".into();
        self.save_pending(&p)?;
        let result = (|| {
            let response = self.auth_request(
                &mut p.session,
                "/account/login_code/",
                &[("code", code.into())],
                Some(&json!({"phone_num":p.phone_encrypted})),
                true,
            )?;
            if response["status"] != "ok" {
                p.status = failure(&response).into();
                self.save_pending(&p)?;
                return Ok(self.public(&p));
            }
            self.complete(&mut p, &response["result"])
        })();
        result.map_err(|e| auth_error(e, id, "login-submit"))
    }
    fn complete(&mut self, p: &mut Pending, result: &Value) -> Result<Value> {
        let mut candidate = p.session.clone();
        if result["pkey"].is_string() {
            let mut bootstrap = result.clone();
            if bootstrap["expire_at"].as_i64().is_none() {
                bootstrap["expire_at"] = json!(now() + TTL);
            }
            candidate.apply(&bootstrap, None)?;
        }
        // The OTP/callback is consumed. Save its candidate before any follow-up
        // reads, so login-status never replays the original exchange.
        p.candidate = Some(candidate);
        p.status = "verifying".into();
        self.save_pending(p)?;
        self.finish_candidate(p)
    }
    fn finish_candidate(&mut self, p: &mut Pending) -> Result<Value> {
        let mut candidate = p
            .candidate
            .clone()
            .ok_or_else(|| protocol_error("Heybox pending login omitted its candidate"))?;
        let expected = candidate.identity().ok();
        let restored =
            self.auth_request(&mut candidate, "/account/restore_login", &[], None, false)?;
        candidate.apply(&checked_result(&restored)?, expected.as_deref())?;
        p.candidate = Some(candidate.clone());
        self.save_pending(p)?;
        self.verify(&mut candidate)?;
        if now() >= p.expires_at {
            self.clear_pending()?;
            return Ok(expired(&p.id));
        }
        self.state.active = Some(candidate.method.clone());
        self.state
            .sessions
            .insert(candidate.method.clone(), candidate.clone());
        // Save the verified account before removing a QR or pending marker.
        self.save()?;
        self.clear_pending()?;
        Ok(authenticated(&candidate, Some(&p.id)))
    }
    fn forget(&mut self) -> Result<Value> {
        if let Some(method) = self.state.active.take() {
            self.state.sessions.remove(&method);
        }
        self.save()?;
        self.clear_pending()?;
        Ok(
            json!({"platform":"blackbox","status":"forgotten","forgotten":true,"remote_logout":false}),
        )
    }
    fn wechat_start(&mut self, p: &mut Pending) -> Result<Value> {
        let mut entry =
            Url::parse("https://api.xiaoheihe.cn/account/wechat/login/v2/web_sso/").unwrap();
        entry
            .query_pairs_mut()
            .append_pair("redirect_url", &format!("{}&src=1", protocol::LOGIN));
        let response = self.api.fetch(&mut p.session, entry.clone(), true)?;
        let mut oauth = redirect(&entry, &response)?;
        if oauth.host_str() != Some("open.weixin.qq.com")
            || oauth.path() != "/connect/qrconnect"
            || oauth.fragment().is_some_and(|s| s != "wechat_redirect")
        {
            return Err(protocol_error(
                "Heybox OAuth entry did not select the official WeChat QR page",
            ));
        }
        oauth.set_fragment(None);
        protocol::oauth_origin(&oauth)?;
        let field = |name: &str| -> Result<String> {
            let values: Vec<_> = oauth
                .query_pairs()
                .filter(|(k, _)| k == name)
                .map(|(_, v)| v.into_owned())
                .collect();
            if values.len() != 1 || values[0].is_empty() {
                return Err(protocol_error(
                    "Heybox OAuth entry has missing or repeated fields",
                ));
            }
            Ok(values[0].clone())
        };
        let callback = field("redirect_uri")?;
        validate_callback(&callback)?;
        let state = field("state")?;
        if state.len() > 2048 {
            return Err(protocol_error("Heybox OAuth state is oversized"));
        }
        let response = self.api.fetch(&mut p.session, oauth, false)?;
        if response.status != 200 {
            return Err(protocol_error("WeChat QR page is unavailable"));
        }
        let page = std::str::from_utf8(&response.body)
            .map_err(|_| protocol_error("WeChat QR page is not UTF-8"))?;
        let image = tags(page, "img")
            .into_iter()
            .filter_map(|attrs| attrs.get("src").cloned())
            .find(|s| {
                s.strip_prefix("/connect/qrcode/")
                    .is_some_and(protocol::opaque)
            })
            .ok_or_else(|| protocol_error("WeChat page omitted its QR image"))?;
        let uuid = image.strip_prefix("/connect/qrcode/").unwrap();
        let polling=regex::Regex::new(r#"var\s+fordevtool\s*=\s*["'](https://long\.open\.weixin\.qq\.com/connect/l/qrconnect\?uuid=[A-Za-z0-9_-]+)"#).unwrap()
            .captures(page).and_then(|c|c.get(1)).map(|s|s.as_str()).ok_or_else(||protocol_error("WeChat page omitted its polling session"))?;
        let poll =
            Url::parse(polling).map_err(|_| protocol_error("WeChat polling session is invalid"))?;
        if poll
            .query_pairs()
            .find(|(k, _)| k == "uuid")
            .map(|(_, v)| v.into_owned())
            .as_deref()
            != Some(uuid)
        {
            return Err(protocol_error("WeChat QR and polling identities differ"));
        }
        let response = self.api.fetch(
            &mut p.session,
            Url::parse(&format!("https://open.weixin.qq.com{image}")).unwrap(),
            false,
        )?;
        let suffix = if response.body.starts_with(b"\x89PNG\r\n\x1a\n") {
            "png"
        } else if response.body.starts_with(b"\xff\xd8\xff") {
            "jpg"
        } else {
            return Err(protocol_error("WeChat QR image format is unsupported"));
        };
        if response.status != 200 || response.body.len() > 1024 * 1024 {
            return Err(protocol_error(
                "WeChat QR image is unavailable or oversized",
            ));
        }
        let path = self.root.join(format!("qr-{}.{}", p.id, suffix));
        storage::atomic_write_controlled(&path, &response.body, &self.control)?;
        p.image_suffix = Some(suffix.into());
        p.oauth_uuid = Some(uuid.into());
        p.oauth_callback = Some(callback);
        p.oauth_state = Some(state);
        p.status = "waiting_scan".into();
        self.save_pending(p)?;
        Ok(self.public(p))
    }
    pub fn status(&mut self, id: &str) -> Result<Value> {
        let Some(mut p) = self.pending(id)? else {
            return Ok(expired(id));
        };
        if p.candidate.is_some() {
            return self
                .finish_candidate(&mut p)
                .map_err(|e| auth_error(e, id, "login-status"));
        }
        if p.session.method == "sms" || TERMINAL.contains(&p.status.as_str()) {
            return Ok(self.public(&p));
        }
        let result = (|| {
            if p.oauth_next.is_some() || p.callback_attempted {
                return self.wechat_finish(&mut p);
            }
            let Some(uuid) = p.oauth_uuid.as_deref() else {
                return Ok(self.public(&p));
            };
            if !protocol::opaque(uuid) {
                return Err(protocol_error("WeChat pending polling identity is invalid"));
            }
            let mut url =
                Url::parse("https://long.open.weixin.qq.com/connect/l/qrconnect").unwrap();
            url.query_pairs_mut()
                .append_pair("uuid", uuid)
                .append_pair("_", &(now() * 1000).to_string());
            if let Some(last) = p.oauth_last.filter(|v| [404, 408].contains(v)) {
                url.query_pairs_mut().append_pair("last", &last.to_string());
            }
            let response = self.api.fetch(&mut p.session, url, false)?;
            let body = std::str::from_utf8(&response.body)
                .map_err(|_| protocol_error("WeChat polling response is not UTF-8"))?;
            let status = regex::Regex::new(r"\bwx_errcode\s*=\s*(\d+)")
                .unwrap()
                .captures(body)
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse::<u16>().ok())
                .filter(|_| response.status == 200)
                .ok_or_else(|| protocol_error("WeChat polling response is invalid"))?;
            p.oauth_last = Some(status);
            match status {
                405 => {
                    let code =
                        regex::Regex::new(r#"\bwx_code\s*=\s*["']([A-Za-z0-9_-]{1,512})["']"#)
                            .unwrap()
                            .captures(body)
                            .and_then(|c| c.get(1))
                            .map(|m| m.as_str())
                            .ok_or_else(|| {
                                protocol_error("WeChat confirmation omitted its exchange code")
                            })?;
                    let callback = p.oauth_callback.as_deref().ok_or_else(|| {
                        protocol_error("WeChat pending login omitted its callback")
                    })?;
                    validate_callback(callback)?;
                    let mut url = Url::parse(callback).unwrap();
                    if url.query_pairs().any(|(k, _)| k == "code" || k == "state") {
                        return Err(protocol_error(
                            "WeChat callback has conflicting exchange fields",
                        ));
                    }
                    url.query_pairs_mut().append_pair("code", code).append_pair(
                        "state",
                        p.oauth_state.as_deref().ok_or_else(|| {
                            protocol_error("WeChat pending login omitted its state")
                        })?,
                    );
                    p.oauth_next = Some(url.into());
                    p.status = "waiting_confirmation".into();
                    self.save_pending(&p)?;
                    return self.wechat_finish(&mut p);
                }
                404 => p.status = "waiting_confirmation".into(),
                408 => {
                    if p.status != "waiting_confirmation" {
                        p.status = "waiting_scan".into();
                    }
                }
                402 | 403 => {
                    p.status = if status == 402 {
                        "expired"
                    } else {
                        "cancelled"
                    }
                    .into();
                    self.clear_pending()?;
                    return Ok(self.public(&p));
                }
                _ => p.status = "failed".into(),
            }
            self.save_pending(&p)?;
            Ok(self.public(&p))
        })();
        result.map_err(|e| auth_error(e, id, "login-status"))
    }
    fn wechat_finish(&mut self, p: &mut Pending) -> Result<Value> {
        for _ in 0..4 {
            if p.callback_attempted {
                break;
            }
            let Some(next) = p.oauth_next.as_deref() else {
                break;
            };
            let url =
                Url::parse(next).map_err(|_| protocol_error("WeChat callback URL is invalid"))?;
            protocol::oauth_origin(&url)?;
            if !matches!(
                (url.host_str(), url.path()),
                (Some("api.xiaoheihe.cn"), protocol::CALLBACK)
                    | (Some("login.xiaoheihe.cn"), "/")
                    | (Some("open.xiaoheihe.cn"), "/zh_cn/workshop")
            ) {
                return Err(protocol_error("WeChat callback changed its service"));
            }
            p.callback_attempted = true;
            self.save_pending(p)?;
            let response = self.api.fetch(&mut p.session, url.clone(), true)?;
            p.oauth_next = if [301, 302, 303, 307, 308].contains(&response.status) {
                Some(redirect(&url, &response)?.into())
            } else if response.status == 200 {
                None
            } else {
                return Err(protocol_error(
                    "WeChat callback returned an unexpected status",
                ));
            };
            p.callback_attempted = false;
            self.save_pending(p)?;
        }
        if p.oauth_next.is_some() && !p.callback_attempted {
            return Ok(self.public(p));
        }
        // A lost callback response may have consumed the code. Only query its
        // existing state; never send that exchange URL again.
        let response = self.auth_request(
            &mut p.session,
            "/account/wechat/state/",
            &[("_notip", "true".into())],
            None,
            false,
        )?;
        self.save_pending(p)?;
        if response["status"] != "ok" {
            p.status = if p.callback_attempted {
                "verification_required"
            } else {
                failure(&response)
            }
            .into();
            self.save_pending(p)?;
            return Ok(self.public(p));
        }
        self.complete(p, &response["result"])
    }
}
fn failure(v: &Value) -> &'static str {
    match v["status"].as_str() {
        Some("show_captcha") => "captcha_required",
        Some("lack_token") => "device_verification_required",
        Some("name_verify" | "need_alipay_verify" | "need_bind_phone" | "need_phone_code") => {
            "verification_required"
        }
        _ => "failed",
    }
}
fn expired(id: &str) -> Value {
    json!({"platform":"blackbox","status":"expired","authenticated":false,"login_id":id})
}
fn authenticated(s: &Session, id: Option<&str>) -> Value {
    json!({"platform":"blackbox","status":"authenticated","authenticated":true,"session_present":true,"api_ready":true,"method":s.method,"login_id":id,"account_id":s.identity,"expires_at":s.expire_at})
}
fn auth_error(error: ToolkitError, id: &str, stage: &str) -> ToolkitError {
    ToolkitError::new(error.code,"Heybox login step did not finish; continue the original login with login-status",error.exit_code)
        .with_details(json!({"login_id":id,"stage":stage,"automatic_retry":false,"next":"publish blackbox session login-status --login-id <original-login-id>"}))
}
fn validate_callback(value: &str) -> Result<()> {
    let url = Url::parse(value).map_err(|_| protocol_error("WeChat callback URL is invalid"))?;
    protocol::oauth_origin(&url)?;
    if url.host_str() != Some("api.xiaoheihe.cn") || url.path() != protocol::CALLBACK {
        return Err(protocol_error("WeChat callback changed its service"));
    }
    Ok(())
}
fn redirect(from: &Url, response: &crate::transport::Response) -> Result<Url> {
    if ![301, 302, 303, 307, 308].contains(&response.status) {
        return Err(protocol_error(
            "Heybox OAuth entry did not return a redirect",
        ));
    }
    let location = response
        .headers
        .get("location")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| protocol_error("Heybox OAuth response omitted its redirect"))?;
    from.join(location)
        .map_err(|_| protocol_error("Heybox OAuth redirect URL is invalid"))
}
fn tags(page: &str, tag: &str) -> Vec<BTreeMap<String, String>> {
    let re = regex::Regex::new(&format!(r"(?i)<{tag}\b[^>]*>")).unwrap();
    let attrs =
        regex::Regex::new(r#"([A-Za-z_][A-Za-z0-9_-]*)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#)
            .unwrap();
    re.find_iter(page)
        .map(|m| {
            attrs
                .captures_iter(m.as_str())
                .map(|c| {
                    let value = c
                        .get(2)
                        .or_else(|| c.get(3))
                        .or_else(|| c.get(4))
                        .unwrap()
                        .as_str();
                    (
                        c[1].to_ascii_lowercase(),
                        value
                            .replace("&quot;", "\"")
                            .replace("&#39;", "'")
                            .replace("&lt;", "<")
                            .replace("&gt;", ">")
                            .replace("&amp;", "&"),
                    )
                })
                .collect()
        })
        .collect()
}
fn web_version(bytes: &[u8]) -> Result<String> {
    let page =
        std::str::from_utf8(bytes).map_err(|_| protocol_error("Heybox login page is not UTF-8"))?;
    Ok(tags(page, "meta")
        .into_iter()
        .find(|t| t.get("name").is_some_and(|s| s == "web_version"))
        .and_then(|t| t.get("content").cloned())
        .unwrap_or_default()
        .chars()
        .take(256)
        .collect())
}
#[cfg(not(test))]
fn device_risk_token() -> Option<String> {
    let root = credentials::app_data(credentials::AppData::Roaming).ok()?;
    let path = root.join("heybox-pc-launcher/config.json");
    storage::guard_path(&root, &path).ok()?;
    let mut file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 8 * 1024 * 1024 {
        return None;
    }
    let value = input::value(&bytes).ok()?;
    let value = value.pointer("/acc_config/xhh_token_id")?.as_str()?;
    (value.len() == 89
        && value.starts_with('B')
        && value.bytes().all(|b| b > 32 && b != 127 && b != b';'))
    .then(|| value.into())
}
#[cfg(test)]
fn device_risk_token() -> Option<String> {
    None
}

#[cfg(test)]
mod tests;
