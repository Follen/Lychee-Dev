//! DD's native signed author transport. Task-session acquisition and resource
//! business remain separate: this capability neither reads desktop credentials
//! nor implicitly logs in, refreshes a session, retries a write or logs out.
use crate::transport::{Http, Origin, secret_header};
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use reqwest::{
    Url,
    header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue, ORIGIN, REFERER, USER_AGENT},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, time::Instant};
mod app_id;
mod backup;
pub mod cc;
mod channel;
mod config;
mod delete;
mod desktop;
mod desktop_cookie;
mod device;
mod forms;
mod installation;
mod plugin;
mod publication;
mod read;
mod saved_credential;
mod session;
pub mod task;
mod upload;
mod wa;
mod wa_write;
pub mod worker;
pub use app_id::request as request_app_id;
pub use backup::{
    BackupCreate, BackupDelete, BackupGenerate, BackupRename, BackupSelection, BackupUpload,
    scan as backup_scan,
};
pub use channel::helper_entry as channel_helper_entry;
pub use config::ConfigWrite;
pub use delete::Delete;
pub use forms::WriteAction;
pub use installation::Installation;
pub use plugin::PluginWrite;
pub use read::{Page, Read, Reference, Resource, life_types, read};
pub use saved_credential::{CredentialKind, SavedCredential};
pub use upload::{ObjectUpload, UploadKind};
pub use wa::{WaIdentity, parse as parse_wa};
pub use wa_write::WaWrite;

const BASE: &str = "https://uiapi.w.163.com/";
const USER_AGENT_VALUE: &str = "Mozilla/5.0 (Windows NT 10.0; WOW64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/108.0.0.0 Safari/537.36 app/df_client dfVersion/100130";

/// Inspect the selected acquisition mode without reading credentials, cookies
/// or loading SDK code. Authentication is performed only by explicit start.
pub fn doctor(control: &OperationControl) -> Result<Value> {
    control.check()?;
    let (installation, source, desktop) = match desktop::select(control)? {
        Some(desktop) => (
            desktop.installation,
            "desktop-cookie",
            json!({"gui":desktop.gui,"browser":desktop.network}),
        ),
        None => (
            Installation::discover(control)?,
            "native-login",
            Value::Null,
        ),
    };
    Ok(
        json!({"platform":"dd","api_origin":BASE,"session_source":source,
        "installation":{"directory":installation.program_dir,"executable":installation.executable,"version":installation.version},
        "desktop":desktop,"authentication_checked":false,"login_performed":false,
        "cookie_read":false,"saved_credentials_read":false,"native_sdk_loaded":false,"sdk_bytes_checked":false,
        "next_command":"lycheedev publish dd session start"}),
    )
}

trait Signer {
    fn sign(&mut self, url: &str, body: Option<&[u8]>, extra: &[(&str, &str)]) -> Result<String>;
    fn close(self: Box<Self>) -> Result<()>;
}
struct NativeSigner(lycheedev_windows::nep_signer::NepSigner);
impl Signer for NativeSigner {
    fn sign(&mut self, url: &str, body: Option<&[u8]>, extra: &[(&str, &str)]) -> Result<String> {
        match body {
            Some(body) => self.0.post(url, body, extra),
            None => self.0.get(url, extra),
        }
        .map_err(native_error)
    }
    fn close(self: Box<Self>) -> Result<()> {
        self.0.close().map_err(native_error)
    }
}
fn native_error(message: String) -> ToolkitError {
    ToolkitError::new("publish.dd_signer", message, 3)
}
struct Clock {
    timestamp: u64,
    sampled: Instant,
}
impl Clock {
    fn now(&self) -> Result<String> {
        self.timestamp
            .checked_add(self.sampled.elapsed().as_secs())
            .map(|value| value.to_string())
            .ok_or_else(|| data("DD server clock exceeded its supported range"))
    }
}

/// Serial HTTP capability composed by the DD session owner. Cookie/token
/// arguments are in-memory capabilities, never public CLI flags or output.
pub struct DdTransport {
    http: Http,
    control: OperationControl,
    signer: Option<Box<dyn Signer>>,
    cookie: String,
    token: String,
    clock: Option<Clock>,
}
impl DdTransport {
    pub(super) fn begin_operation(&mut self, control: &OperationControl) -> Result<()> {
        control.check()?;
        self.http.begin_operation(control)?;
        self.control = control.clone();
        Ok(())
    }
    pub fn open(
        sdk: &Path,
        cookie: String,
        token: String,
        control: &OperationControl,
    ) -> Result<Self> {
        validate_credentials(&cookie, &token)?;
        control.check()?;
        let http = Http::new(control)?;
        let signer = NativeSigner(
            lycheedev_windows::nep_signer::NepSigner::open(sdk).map_err(native_error)?,
        );
        control.check()?;
        Ok(Self {
            http,
            control: control.clone(),
            signer: Some(Box::new(signer)),
            cookie,
            token,
            clock: None,
        })
    }
    /// Explicit author-service exchange only. The task owner supplies a desktop
    /// cookie from its selected session mode; this never attempts a CC login.
    pub fn login(&mut self) -> Result<Value> {
        if !self.token.is_empty() {
            return Err(ToolkitError::new(
                "publish.dd_session",
                "DD author session is already authenticated",
                3,
            ));
        }
        self.synchronize_clock()?;
        let mut payload = self.request_once("/login/dflogin", &[], None, false)?;
        if signature_rejected(&payload) {
            self.synchronize_clock()?;
            payload = self.request_once("/login/dflogin", &[], None, false)?;
        }
        if payload.get("code").and_then(Value::as_i64) != Some(200) {
            return Err(rejected(&payload, false));
        }
        let result = payload
            .get("result")
            .and_then(Value::as_object)
            .ok_or_else(|| data("DD author login omitted its result"))?;
        let token = result
            .get("token")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| data("DD author login omitted its token"))?;
        validate_credentials(&self.cookie, token)
            .map_err(|_| data("DD author login returned invalid credentials"))?;
        let id = match result.get("uid") {
            Some(Value::Number(value)) if value.as_u64().is_some_and(|id| id > 0) => {
                value.to_string()
            }
            Some(Value::String(value)) if !value.trim().is_empty() => value.clone(),
            _ => return Err(data("DD author login omitted its author identity")),
        };
        self.token = token.to_owned();
        // Only an identity projection leaves the login boundary. In particular
        // no result token, cookie, raw error message or signed URL is returned.
        Ok(json!({"author_id":id,"nickname":result.get("nickname").and_then(Value::as_str)}))
    }
    pub fn get(&mut self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        self.authenticated()?;
        let mut payload = self.request_once(path, query, None, false)?;
        if signature_rejected(&payload) {
            self.synchronize_clock()?;
            payload = self.request_once(path, query, None, false)?;
        }
        business_result(payload, false)
    }
    /// POST dependency queries are explicitly distinguished from mutations for
    /// uncertainty reporting. Neither form is retried or causes another login.
    /// The business caller records mutation intent before calling with true.
    pub fn post(
        &mut self,
        path: &str,
        query: &[(&str, String)],
        body: &Value,
        mutation: bool,
    ) -> Result<Value> {
        self.authenticated()?;
        let bytes =
            serde_json::to_vec(body).map_err(|_| data("DD request body could not be encoded"))?;
        let payload = self.request_once(path, query, Some(bytes), mutation)?;
        business_result(payload, mutation)
    }
    fn authenticated(&self) -> Result<()> {
        if self.token.is_empty() {
            Err(ToolkitError::new(
                "publish.dd_session",
                "DD author session requires an explicit login",
                3,
            ))
        } else {
            Ok(())
        }
    }
    fn synchronize_clock(&mut self) -> Result<()> {
        self.control.check()?;
        let value = self
            .http
            .get(Origin::DdUi, "/server/ts", &[], base_headers())?;
        if value.get("code").and_then(Value::as_i64) != Some(200) {
            return Err(data("DD server time request was rejected"));
        }
        let value = value
            .pointer("/result/ts")
            .ok_or_else(|| data("DD server clock is missing"))?;
        let timestamp = value
            .as_u64()
            .or_else(|| value.as_str().and_then(|v| v.parse().ok()))
            .filter(|timestamp| *timestamp > 0 && *timestamp < u64::MAX - 86400)
            .ok_or_else(|| data("DD server clock is not a positive integer"))?;
        self.clock = Some(Clock {
            timestamp,
            sampled: Instant::now(),
        });
        Ok(())
    }
    fn request_once(
        &mut self,
        path: &str,
        query: &[(&str, String)],
        body: Option<Vec<u8>>,
        mutation: bool,
    ) -> Result<Value> {
        let prepared = (|| {
            let original = route(path, query)?;
            self.control.check()?;
            if self.clock.is_none() {
                self.synchronize_clock()?;
            }
            // One sample supplies both signature extraArgs and HTTP headers,
            // even when signing crosses a second. No local-clock fallback.
            let timestamp = self.clock.as_ref().expect("clock synchronized").now()?;
            let extra = [
                ("x-w163-token", self.token.as_str()),
                ("x-timestamp", timestamp.as_str()),
            ];
            let signed = self
                .signer
                .as_mut()
                .expect("open transport owns signer")
                .sign(original.as_str(), body.as_deref(), &extra)?;
            let signed = signed_target(&original, &signed)?;
            self.control.check()?;
            let mut headers = base_headers();
            headers.extend(secret_header("x-w163-token", &self.token)?);
            headers.extend(secret_header(
                "cookie",
                &format!("NTES_YD_SESS={}", self.cookie),
            )?);
            headers.insert(
                "x-timestamp",
                HeaderValue::from_str(&timestamp).expect("decimal timestamp"),
            );
            Ok((signed, headers))
        })();
        let (signed, headers) =
            prepared.map_err(|error| submission(error, mutation, "not_sent"))?;
        self.http
            .signed_json(Origin::DdUi, signed, headers, body, mutation)
    }
    pub fn close(mut self) -> Result<()> {
        self.signer
            .take()
            .expect("open transport owns signer")
            .close()
    }
}
fn base_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/json;charset=UTF-8"),
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/plain, */*"),
    );
    headers.insert(ORIGIN, HeaderValue::from_static("https://dev.cc.163.com"));
    headers.insert(REFERER, HeaderValue::from_static("https://dev.cc.163.com/"));
    headers
}
fn validate_credentials(cookie: &str, token: &str) -> Result<()> {
    if cookie.trim().is_empty()
        || cookie.len() > 65536
        || token.len() > 65536
        || cookie.contains([';', '\r', '\n', '\0'])
        || token.contains(['\r', '\n', '\0'])
    {
        return Err(ToolkitError::new(
            "publish.dd_credentials",
            "DD session credentials are invalid",
            3,
        ));
    }
    let _ = secret_header("cookie", cookie)?;
    let _ = secret_header("x-w163-token", token)?;
    Ok(())
}
fn route(path: &str, query: &[(&str, String)]) -> Result<Url> {
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.contains(['?', '#', '\\'])
        || path.split('/').any(|part| part == "." || part == "..")
        || path
            .bytes()
            .any(|byte| byte <= b' ' || byte >= 127 || byte == b'%')
    {
        return Err(ToolkitError::new(
            "publish.dd_route",
            "invalid DD author route",
            8,
        ));
    }
    let mut url = Url::parse(BASE).expect("fixed DD author origin");
    url.set_path(path);
    if !query.is_empty() {
        url.query_pairs_mut()
            .extend_pairs(query.iter().map(|(key, value)| (*key, value.as_str())));
    }
    Ok(url)
}
fn query_counts(url: &Url) -> BTreeMap<(String, String), usize> {
    let mut counts = BTreeMap::new();
    for (key, value) in url.query_pairs() {
        *counts
            .entry((key.into_owned(), value.into_owned()))
            .or_default() += 1;
    }
    counts
}
fn signed_target(original: &Url, signed: &str) -> Result<Url> {
    let error = || {
        ToolkitError::new(
            "publish.dd_signed_target",
            "DD signature changed its selected route or arguments",
            3,
        )
    };
    let url = Url::parse(signed).map_err(|_| error())?;
    if url.origin() != original.origin()
        || url.path() != original.path()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(error());
    }
    let expected = query_counts(original);
    let actual = query_counts(&url);
    for ((key, value), count) in &expected {
        if actual.get(&(key.clone(), value.clone())) != Some(count) {
            return Err(error());
        }
    }
    for ((key, value), count) in &actual {
        if expected.keys().any(|(name, _)| name == key)
            && expected.get(&(key.clone(), value.clone())) != Some(count)
        {
            return Err(error());
        }
    }
    Ok(url)
}
fn signature_rejected(value: &Value) -> bool {
    value.get("code").and_then(Value::as_i64) == Some(409)
        && value
            .get("msg")
            .or_else(|| value.get("message"))
            .and_then(Value::as_str)
            .is_some_and(|v| v.trim() == "签名无效")
}
fn business_result(payload: Value, mutation: bool) -> Result<Value> {
    if !payload.is_object() {
        return Err(submission(
            data("DD author response is not an object"),
            mutation,
            "uncertain",
        ));
    }
    match payload.get("code") {
        None => Ok(payload),
        Some(value) if value.as_i64() == Some(0) => Ok(payload),
        Some(value) if value.as_i64().is_some() => Err(rejected(&payload, mutation)),
        _ => Err(submission(
            data("DD author response has an invalid business code"),
            mutation,
            "uncertain",
        )),
    }
}
fn rejected(value: &Value, mutation: bool) -> ToolkitError {
    // Server messages may echo tokens, WA strings, contact details or signed
    // object URLs. Only numeric codes cross the error boundary.
    let code = value.get("code").and_then(Value::as_i64);
    ToolkitError::new("publish.dd_rejected", "DD author service rejected the request", 5)
        .with_details(json!({"business_code":code,"submission":if mutation {"rejected"} else {"not_applicable"},"replay_allowed":false}))
}
fn data(message: &str) -> ToolkitError {
    ToolkitError::new("publish.dd_data", message, 5)
}
fn submission(mut error: ToolkitError, mutation: bool, state: &str) -> ToolkitError {
    if mutation {
        if !error.details.is_object() {
            error.details = json!({});
        }
        error.details["submission"] = json!(state);
        error.details["replay_allowed"] = json!(false);
    }
    error
}

#[cfg(test)]
mod tests;
