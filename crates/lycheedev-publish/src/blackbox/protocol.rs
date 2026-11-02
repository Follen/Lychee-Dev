use super::*;
use crate::transport::{Http, Origin, Response, secret_header};
use md5::{Digest, Md5};
use reqwest::{
    Url,
    header::{CONTENT_TYPE, HeaderValue, ORIGIN, REFERER, SET_COOKIE, USER_AGENT},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub(super) const LOGIN: &str = "https://login.xiaoheihe.cn/?origin=heybox_open&redirect_url=https%3A%2F%2Fopen.xiaoheihe.cn%2Fzh_cn%2Fworkshop";
pub(super) const CALLBACK: &str = "/account/wechat/login_redirect/v2/web_sso/";
const ALPHABET: &[u8] = b"AB45STUVWZEFGJ6CH01D237IXYPQRKLMN89";
const IDENTITY: &[&str] = &["heybox_id", "user_heybox_id", "pkey", "user_pkey"];
const DOMAINS: &[&str] = &[
    "xiaoheihe.cn",
    "api.xiaoheihe.cn",
    "login.xiaoheihe.cn",
    "open.xiaoheihe.cn",
    "workshopapi.xiaoheihe.cn",
    "weixin.qq.com",
    "open.weixin.qq.com",
    "long.open.weixin.qq.com",
];

#[derive(Clone, Serialize, Deserialize, Default)]
pub(super) struct Session {
    pub method: String,
    pub device_id: String,
    pub web_version: String,
    pub cookies: Vec<Cookie>,
    pub identity: Option<String>,
    pub expire_at: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Cookie {
    name: String,
    value: String,
    domain: String,
    path: String,
    host_only: bool,
    secure: bool,
    expires: Option<i64>,
}
impl Cookie {
    fn validate(&self) -> Result<()> {
        if self.name.is_empty()
            || !self
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            || self.value.bytes().any(|b| b < 32 || b == 127 || b == b';')
            || !DOMAINS.contains(&self.domain.as_str())
            || !self.path.starts_with('/')
            || self.path.chars().any(char::is_control)
        {
            return Err(protocol_error("Heybox cookie has an invalid name or scope"));
        }
        Ok(())
    }
    fn applies(&self, url: &Url) -> bool {
        let host = url.host_str().unwrap_or_default();
        (host == self.domain || (!self.host_only && host.ends_with(&format!(".{}", self.domain))))
            && (!self.secure || url.scheme() == "https")
            && self.expires.is_none_or(|t| t > now())
            && (url.path() == self.path
                || (url.path().starts_with(&self.path)
                    && (self.path.ends_with('/')
                        || url.path().as_bytes().get(self.path.len()) == Some(&b'/'))))
    }
}
impl Session {
    pub fn identity(&self) -> Result<String> {
        let mut uid = BTreeSet::new();
        let mut keys = BTreeSet::new();
        let mut scopes = BTreeSet::new();
        if self.cookies.len() > 256 {
            return Err(protocol_error("Heybox cookie store is oversized"));
        }
        for c in &self.cookies {
            c.validate()?;
            if !scopes.insert((&c.domain, &c.path, &c.name)) {
                return Err(protocol_error("duplicate Heybox cookie scope"));
            }
            if IDENTITY.contains(&c.name.as_str()) {
                if c.domain != "xiaoheihe.cn" || c.path != "/" {
                    return Err(protocol_error(
                        "Heybox identity cookie has an invalid scope",
                    ));
                }
                if c.name.ends_with("pkey") {
                    keys.insert(c.value.as_str());
                } else {
                    uid.insert(c.value.as_str());
                }
            }
        }
        if uid.len() != 1 || keys.len() != 1 || keys.first().is_none_or(|s| s.is_empty()) {
            return Err(login_required());
        }
        let id = *uid.first().unwrap();
        if id.is_empty()
            || id.len() > 32
            || !id.bytes().all(|b| b.is_ascii_digit())
            || self.identity.as_ref().is_some_and(|s| s != id)
        {
            return Err(protocol_error(
                "Heybox session identity does not match its cookies",
            ));
        }
        Ok(id.into())
    }
    pub fn apply(&mut self, result: &Value, expected: Option<&str>) -> Result<()> {
        let uid = result
            .pointer("/profile/heybox_id")
            .or_else(|| result.pointer("/account_detail/userid"))
            .and_then(identifier)
            .ok_or_else(|| protocol_error("Heybox response omitted its account identity"))?;
        let key = result["pkey"]
            .as_str()
            .filter(|s| !s.is_empty() && !s.bytes().any(|b| b < 32 || b == 127 || b == b';'))
            .ok_or_else(|| protocol_error("Heybox response omitted a valid session key"))?;
        let expiry = result["expire_at"]
            .as_i64()
            .filter(|e| *e > now() && *e <= now() + 366 * 86400)
            .ok_or_else(|| protocol_error("Heybox response omitted a valid server expiry"))?;
        if expected.is_some_and(|id| id != uid) {
            return Err(ToolkitError::new(
                "publish.blackbox_identity",
                "Heybox renewal changed the selected account",
                3,
            ));
        }
        if uid.len() > 32 || !uid.bytes().all(|b| b.is_ascii_digit()) {
            return Err(protocol_error("Heybox account identity is invalid"));
        }
        self.cookies
            .retain(|c| !IDENTITY.contains(&c.name.as_str()));
        for name in IDENTITY {
            self.cookies.push(Cookie {
                name: (*name).into(),
                value: if name.ends_with("pkey") {
                    key.into()
                } else {
                    uid.clone()
                },
                domain: "xiaoheihe.cn".into(),
                path: "/".into(),
                host_only: false,
                secure: true,
                expires: Some(expiry),
            });
        }
        self.identity = Some(uid);
        self.expire_at = Some(expiry);
        Ok(())
    }
    fn header(&self, url: &Url) -> Result<String> {
        let mut rows = Vec::new();
        for c in &self.cookies {
            c.validate()?;
            if c.applies(url) {
                rows.push(format!("{}={}", c.name, c.value));
            }
        }
        Ok(rows.join("; "))
    }
    fn update_cookies(&mut self, url: &Url, response: &Response) -> Result<()> {
        let host = url.host_str().unwrap();
        for header in response.headers.get_all(SET_COOKIE) {
            let raw = header
                .to_str()
                .map_err(|_| protocol_error("Heybox returned an invalid cookie"))?;
            let mut parts = raw.split(';');
            let (name, value) = parts
                .next()
                .unwrap_or_default()
                .trim()
                .split_once('=')
                .ok_or_else(|| protocol_error("Heybox returned an invalid cookie"))?;
            let parent = url
                .path()
                .rsplit_once('/')
                .map(|(p, _)| p)
                .filter(|p| !p.is_empty())
                .unwrap_or("/");
            let mut cookie = Cookie {
                name: name.into(),
                value: value.into(),
                domain: host.into(),
                path: parent.into(),
                host_only: true,
                secure: false,
                expires: None,
            };
            let mut max_age = None;
            for part in parts {
                let (key, val) = part.trim().split_once('=').unwrap_or((part.trim(), ""));
                match key.to_ascii_lowercase().as_str() {
                    "domain" => {
                        cookie.domain = val.trim_start_matches('.').to_ascii_lowercase();
                        cookie.host_only = false;
                    }
                    "path" if val.starts_with('/') => cookie.path = val.into(),
                    "secure" => cookie.secure = true,
                    "max-age" => max_age = val.parse::<i64>().ok().map(|n| now().saturating_add(n)),
                    "expires" => {
                        cookie.expires = chrono::DateTime::parse_from_rfc2822(val)
                            .ok()
                            .map(|t| t.timestamp())
                            .or_else(|| {
                                chrono::NaiveDateTime::parse_from_str(
                                    val,
                                    "%a, %d-%b-%Y %H:%M:%S GMT",
                                )
                                .ok()
                                .map(|t| t.and_utc().timestamp())
                            })
                    }
                    _ => {}
                }
            }
            if let Some(expiry) = max_age {
                cookie.expires = Some(expiry);
            }
            cookie.validate()?;
            if host != cookie.domain && !host.ends_with(&format!(".{}", cookie.domain)) {
                return Err(protocol_error(
                    "Heybox response set a cookie for another service",
                ));
            }
            self.cookies.retain(|c| {
                (c.domain.as_str(), c.path.as_str(), c.name.as_str())
                    != (
                        cookie.domain.as_str(),
                        cookie.path.as_str(),
                        cookie.name.as_str(),
                    )
            });
            if cookie.expires.is_none_or(|t| t > now()) {
                self.cookies.push(cookie);
            }
            if self.cookies.len() > 256 {
                return Err(protocol_error("Heybox returned too many cookies"));
            }
        }
        Ok(())
    }
}
pub(super) fn identifier(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        Value::Number(n) if n.is_u64() || n.is_i64() => Some(n.to_string()),
        _ => None,
    }
}
pub(super) fn sign(path: &str, timestamp: i64, nonce: &str) -> String {
    fn map(s: &str, size: usize) -> Vec<u8> {
        s.bytes().map(|c| ALPHABET[c as usize % size]).collect()
    }
    fn p(v: u8) -> u8 {
        v.wrapping_shl(1) ^ if v & 128 != 0 { 27 } else { 0 }
    }
    fn h(v: u8) -> u8 {
        p(v) ^ v
    }
    fn q(v: u8) -> u8 {
        h(p(v))
    }
    fn d(v: u8) -> u8 {
        q(h(p(v)))
    }
    fn m(v: u8) -> u8 {
        d(v) ^ q(v) ^ h(v)
    }
    let path = format!(
        "/{}/",
        path.split('/')
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join("/")
    );
    let parts = [
        map(&(timestamp + 1).to_string(), ALPHABET.len() - 2),
        map(&path, ALPHABET.len()),
        map(nonce, ALPHABET.len()),
    ];
    let mut source = Vec::new();
    for i in 0..parts.iter().map(Vec::len).max().unwrap() {
        for part in &parts {
            if let Some(c) = part.get(i) {
                source.push(*c);
            }
        }
    }
    let digest = hex::encode(Md5::digest(&source[..source.len().min(20)]));
    let v = &digest.as_bytes()[26..];
    let (a, b, c, dv) = (v[0], v[1], v[2], v[3]);
    let mixed = [
        m(a) ^ d(b) ^ q(c) ^ h(dv),
        h(a) ^ m(b) ^ d(c) ^ q(dv),
        q(a) ^ h(b) ^ m(c) ^ d(dv),
        d(a) ^ q(b) ^ h(c) ^ m(dv),
        v[4],
        v[5],
    ];
    format!(
        "{}{:02}",
        String::from_utf8(map(&digest[..5], ALPHABET.len() - 4)).unwrap(),
        mixed.iter().map(|v| *v as u32).sum::<u32>() % 100
    )
}

pub(super) struct Api {
    pub http: Http,
}
impl Api {
    pub fn new(control: &OperationControl) -> Result<Self> {
        Ok(Self {
            http: Http::direct(control)?,
        })
    }
    pub fn fetch(&self, session: &mut Session, url: Url, mutation: bool) -> Result<Response> {
        let origin = oauth_origin(&url)?;
        self.send(session, origin, url, None, mutation, None)
    }
    fn send(
        &self,
        session: &mut Session,
        origin: Origin,
        url: Url,
        body: Option<Vec<u8>>,
        mutation: bool,
        risk: Option<&str>,
    ) -> Result<Response> {
        let mut cookie = session.header(&url)?;
        if let Some(risk) = risk {
            if !cookie.is_empty() {
                cookie.push_str("; ");
            }
            cookie.push_str(&format!("x_xhh_tokenid={risk}"));
        }
        let mut headers = secret_header("cookie", &cookie)?;
        headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/142.0.0.0 Safari/537.36"));
        headers.insert(
            ORIGIN,
            HeaderValue::from_static("https://login.xiaoheihe.cn"),
        );
        headers.insert(REFERER, HeaderValue::from_static(LOGIN));
        if body.is_some() {
            headers.insert(
                CONTENT_TYPE,
                HeaderValue::from_static("application/x-www-form-urlencoded;charset=UTF-8"),
            );
        }
        let seconds = match url.host_str() {
            Some("long.open.weixin.qq.com") => 2,
            Some("workshopapi.xiaoheihe.cn") => 30,
            _ => 5,
        };
        let result = self.http.signed_response(
            origin,
            url.clone(),
            headers,
            body,
            mutation,
            std::time::Duration::from_secs(seconds),
        )?;
        session.update_cookies(&url, &result)?;
        Ok(result)
    }
    pub fn request(
        &self,
        session: &mut Session,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
        mutation: bool,
        risk: Option<&str>,
    ) -> Result<Value> {
        let auth = matches!(
            (path, body.is_some()),
            ("/account/restore_login", false)
                | ("/account/get_login_code/", true)
                | ("/account/login_code/", true)
                | ("/account/wechat/state/", false)
        );
        let business = matches!(
            (path, body.is_some()),
            ("/wow/v2/dynamic_conf/", false)
                | ("/wow/open_platform/module/list/", false)
                | ("/wow/open_platform/module/detail/", false)
                | ("/wow/open_platform/module_version/list/", false)
                | ("/wow/get_user_configuration_backup_list/", true)
                | ("/wow/v2/share/list/", true)
                | ("/wow/v2/share/detail/", true)
                | ("/wow/v2/get_user_info/", true)
                | ("/wow/get_wa_list/", true)
                | ("/wow/get_wa_detail/", true)
                | ("/wow/get_wa_tag_list/", true)
                | ("/wow/open_platform/module/update/", true)
                | ("/wow/open_platform/module_version/upsert/", true)
                | ("/wow/open_platform/module_version/delete/", true)
                | ("/wow/cos/upload/token/", true)
                | ("/wow/create_config_share/", true)
                | ("/wow/update_config_share/", true)
                | ("/wow/create_wa/", true)
                | ("/wow/update_wa/", true)
                | ("/wow/create_backup/", true)
                | ("/wow/get_user_backup_credential/", true)
                | ("/wow/save_user_configuration/", true)
                | ("/wow/rename_user_configuration_backup/", true)
                | ("/wow/remove_user_configuration_backup/", true)
        );
        if !auth && !business {
            return Err(protocol_error("unsupported Heybox API method or path"));
        }
        let origin = if auth {
            Origin::HeyboxAccount
        } else {
            Origin::HeyboxWorkshop
        };
        let mut url = Url::parse(if auth {
            "https://api.xiaoheihe.cn/"
        } else {
            "https://workshopapi.xiaoheihe.cn/"
        })
        .unwrap();
        url.set_path(path);
        let timestamp = now();
        let nonce = random()?.to_ascii_uppercase();
        let id = session
            .identity
            .as_deref()
            .or_else(|| {
                session
                    .cookies
                    .iter()
                    .find(|c| c.name == "heybox_id" && c.domain == "xiaoheihe.cn" && c.path == "/")
                    .map(|c| c.value.as_str())
            })
            .unwrap_or("");
        {
            let mut params = url.query_pairs_mut();
            for (k, v) in [
                ("app", "heybox"),
                ("heybox_id", id),
                ("os_type", "web"),
                ("x_app", "heybox_website"),
                ("x_client_type", "weboutapp"),
                ("x_os_type", "Windows"),
                ("web_version", session.web_version.as_str()),
                ("device_id", session.device_id.as_str()),
                ("version", "999.0.4"),
            ] {
                params.append_pair(k, v);
            }
            params
                .append_pair("hkey", &sign(path, timestamp, &nonce))
                .append_pair("_time", &timestamp.to_string())
                .append_pair("nonce", &nonce);
            if let Some(risk) = risk {
                params.append_pair("x_xhh_tokenid", risk);
            }
            for (k, v) in query {
                if [
                    "app",
                    "heybox_id",
                    "os_type",
                    "x_app",
                    "x_client_type",
                    "x_os_type",
                    "web_version",
                    "device_id",
                    "version",
                    "hkey",
                    "_time",
                    "nonce",
                    "x_xhh_tokenid",
                ]
                .contains(k)
                {
                    return Err(protocol_error(
                        "caller tried to replace Heybox authentication fields",
                    ));
                }
                params.append_pair(k, v);
            }
        }
        let body = body.map(form).transpose()?;
        let response = self.send(session, origin, url, body, mutation, risk)?;
        if response.status != 200 {
            return Err(protocol_error(
                "Heybox API returned a redirect instead of a result",
            ));
        }
        crate::input::value(&response.body).and_then(|v| {
            if v.is_object() {
                Ok(v)
            } else {
                Err(protocol_error("Heybox API result must be an object"))
            }
        })
    }
    pub fn data(
        &self,
        session: &mut Session,
        path: &str,
        query: &[(&str, String)],
        post: bool,
    ) -> Result<Value> {
        let result = self.request(
            session,
            path,
            query,
            post.then(|| json!({})).as_ref(),
            false,
            None,
        )?;
        checked_result(&result)
    }
    pub fn post(
        &self,
        session: &mut Session,
        path: &str,
        query: &[(&str, String)],
        body: &Value,
    ) -> Result<Value> {
        let result = self.request(session, path, query, Some(body), true, None)?;
        if result["status"] == "ok" && result.get("result").is_none_or(Value::is_null) {
            return Ok(json!({}));
        }
        checked_result(&result).map_err(|mut error| {
            if error.code == "publish.blackbox_rejected"
                || error.code == "publish.blackbox_login_required"
            {
                error.details["submission"] = json!("rejected");
            }
            error
        })
    }
}
pub(super) fn checked_result(value: &Value) -> Result<Value> {
    match value["status"].as_str() {
        Some("ok") => value.get("result").filter(|v| v.is_object()).cloned().ok_or_else(|| protocol_error("Heybox API result is incomplete")),
        Some("login"|"relogin") => Err(login_required()),
        status => Err(ToolkitError::new("publish.blackbox_rejected", "Heybox rejected the request", 5).with_details(json!({"business_code":status.filter(|s| s.len() <= 64 && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))}))),
    }
}
fn form(value: &Value) -> Result<Vec<u8>> {
    let object = value
        .as_object()
        .ok_or_else(|| protocol_error("Heybox form must be an object"))?;
    let mut pairs = reqwest::Url::parse("https://unused.invalid/").unwrap();
    {
        let mut query = pairs.query_pairs_mut();
        for (k, v) in object {
            match v {
                Value::String(s) => {
                    query.append_pair(k, s);
                }
                Value::Number(n) => {
                    query.append_pair(k, &n.to_string());
                }
                Value::Array(items) => {
                    for item in items {
                        let text = item
                            .as_str()
                            .map(str::to_owned)
                            .or_else(|| item.is_number().then(|| item.to_string()))
                            .ok_or_else(|| {
                                protocol_error("Heybox form arrays must contain text or numbers")
                            })?;
                        query.append_pair(k, &text);
                    }
                }
                _ => return Err(protocol_error("Heybox form field must be text or a number")),
            }
        }
    }
    Ok(pairs.query().unwrap_or_default().as_bytes().to_vec())
}
pub(super) fn oauth_origin(url: &Url) -> Result<Origin> {
    if url.scheme() != "https"
        || url.port().is_some_and(|p| p != 443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(protocol_error(
            "Heybox OAuth URL is outside its official HTTPS routes",
        ));
    }
    match (url.host_str().unwrap_or_default(), url.path()) {
        ("api.xiaoheihe.cn", "/account/wechat/login/v2/web_sso/" | CALLBACK) => {
            Ok(Origin::HeyboxAccount)
        }
        ("login.xiaoheihe.cn", "/") => Ok(Origin::HeyboxLogin),
        ("open.xiaoheihe.cn", "/zh_cn/workshop") => Ok(Origin::HeyboxOpen),
        ("open.weixin.qq.com", "/connect/qrconnect") => Ok(Origin::WechatOpen),
        ("open.weixin.qq.com", p) if p.strip_prefix("/connect/qrcode/").is_some_and(opaque) => {
            Ok(Origin::WechatOpen)
        }
        ("long.open.weixin.qq.com", "/connect/l/qrconnect") => Ok(Origin::WechatPoll),
        _ => Err(protocol_error(
            "Heybox OAuth URL is outside its official routes",
        )),
    }
}
pub(super) fn opaque(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 200
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
