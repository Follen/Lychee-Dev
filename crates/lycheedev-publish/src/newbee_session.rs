//! Reuse the CURRENT user's NewBeeBox desktop session through fixed services.
//! No credentials are accepted in argv or returned by this module.
use crate::transport::{Http, Origin, secret_header};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use lycheedev_core::{Result, ToolkitError, control::OperationControl, storage};
use lycheedev_windows::credentials::{self, AppData};
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{Value, json};
use std::{
    fs,
    io::Read,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) struct CreatorSession {
    store: PathBuf,
    headers: Option<HeaderMap>,
    desktop: Option<HeaderMap>,
}
impl CreatorSession {
    pub(crate) fn open(control: &OperationControl) -> Result<Self> {
        control.check()?;
        let root = credentials::app_data(AppData::Roaming)
            .map_err(|_| auth("Windows current-user Roaming AppData is unavailable"))?;
        let store = root.join("NewBeeBox").join("auth-store");
        storage::guard_path(&root, &store)?;
        if !store.is_dir() {
            return Err(auth(
                "NewBeeBox desktop login state is missing; sign in with the desktop client",
            ));
        }
        Ok(Self {
            store,
            headers: None,
            desktop: None,
        })
    }
    fn read(&self, name: &str, optional: bool, control: &OperationControl) -> Result<String> {
        control.check()?;
        let path = self.store.join(name);
        storage::guard_path(&self.store, &path)?;
        let mut file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(error) if optional && error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(String::new());
            }
            Err(_) => {
                return Err(auth(
                    "NewBeeBox desktop login state is missing; sign in with the desktop client",
                ));
            }
        };
        let mut bytes = Vec::new();
        (&mut file)
            .take(65537)
            .read_to_end(&mut bytes)
            .map_err(|_| auth("cannot read NewBeeBox desktop login state"))?;
        if bytes.len() > 65536 {
            return Err(auth(
                "NewBeeBox desktop credential file exceeds its format limit",
            ));
        }
        control.check()?;
        String::from_utf8(bytes)
            .map(|s| s.trim().to_owned())
            .map_err(|_| auth("NewBeeBox desktop credential file is not valid UTF-8"))
    }
    pub(crate) fn desktop_headers(
        &mut self,
        http: &Http,
        control: &OperationControl,
    ) -> Result<HeaderMap> {
        if let Some(headers) = &self.desktop {
            control.check()?;
            return Ok(headers.clone());
        }
        let mut access = self.read("access-token", false, control)?;
        let mut refresh = self.read("refresh-token", false, control)?;
        let mut proof = self.read("device-proof", true, control)?;
        if !fresh(&access, unix_seconds()?) {
            if refresh.is_empty() {
                return Err(auth(
                    "NewBeeBox refresh token is missing; sign in with the desktop client",
                ));
            }
            let host = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows".into());
            let mut fields = vec![
                ("client_id", "nbb-desktop"),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh.as_str()),
                ("device_name", host.as_str()),
                ("device_type", "desktop"),
            ];
            if !proof.is_empty() {
                fields.push(("device_proof", proof.as_str()));
            }
            let payload = http.form(
                Origin::NewbeeNext,
                "/auth/connect/token",
                HeaderMap::new(),
                &fields,
            )?;
            let new_access = secret(&payload, "access_token")?;
            if let Some(value) = payload
                .get("refresh_token")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                refresh = value.to_owned();
            }
            if let Some(value) = payload
                .get("device_proof")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                proof = value.to_owned();
            }
            access = new_access;
            for (name, value) in [
                ("access-token", &access),
                ("refresh-token", &refresh),
                ("device-proof", &proof),
            ] {
                control.check()?;
                if !value.is_empty() {
                    storage::atomic_write(&self.store.join(name), value.as_bytes())
                        .map_err(|_| auth("cannot save the refreshed NewBeeBox desktop session"))?;
                }
            }
        }
        let mut handoff_headers = secret_header("authorization", &format!("Bearer {access}"))?;
        handoff_headers.insert("boxversion", HeaderValue::from_static("1.1.17"));
        handoff_headers.insert("accept-language", HeaderValue::from_static("zh-CN"));
        self.desktop = Some(handoff_headers.clone());
        Ok(handoff_headers)
    }
    pub(crate) fn headers(&mut self, http: &Http, control: &OperationControl) -> Result<HeaderMap> {
        if let Some(headers) = &self.headers {
            control.check()?;
            return Ok(headers.clone());
        }
        let handoff_headers = self.desktop_headers(http, control)?;
        let payload = http.post(
            Origin::NewbeeCreator,
            "/v3/user/auth2web",
            handoff_headers,
            &json!({}),
            true,
        )?;
        let handoff = secret(&data(payload)?, "code")?;
        let mut headers = HeaderMap::new();
        headers.insert("appid", HeaderValue::from_static("6"));
        headers.insert("accept-language", HeaderValue::from_static("zh-CN"));
        let payload = http.post(
            Origin::NewbeeCreator,
            "/v3/user/exchange_web_code",
            headers.clone(),
            &json!({"code":handoff}),
            true,
        )?;
        let exchanged = data(payload)?;
        let author = secret(&exchanged, "token")?;
        headers.extend(secret_header("token", &author)?);
        if let Some(initial) = exchanged
            .get("jwtToken")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            headers.extend(secret_header(
                "authorization",
                &format!("Bearer {initial}"),
            )?);
        }
        let payload = http.post(
            Origin::NewbeeCreator,
            "/v3/user/refresh_web_resource_token",
            headers.clone(),
            &json!({}),
            true,
        )?;
        let resource = secret(&data(payload)?, "resource_token")?;
        headers.extend(secret_header(
            "authorization",
            &format!("Bearer {resource}"),
        )?);
        self.headers = Some(headers.clone());
        Ok(headers)
    }
    /// OIDC `sub` partitions local receipts across desktop accounts. It is
    /// metadata for the original token, never authorization for a remote write.
    /// The fixed public discovery declares this claim; do not follow its
    /// advertised internal issuer/userinfo addresses or persist token hashes.
    pub(crate) fn subject(&mut self, http: &Http, control: &OperationControl) -> Result<String> {
        let headers = self.desktop_headers(http, control)?;
        let missing = || {
            auth(
                "NewBeeBox desktop access token has no stable OIDC subject; sign in with a compatible desktop client",
            )
        };
        let token = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or_else(missing)?;
        let payload = token
            .split('.')
            .nth(1)
            .and_then(|value| URL_SAFE_NO_PAD.decode(value.trim_end_matches('=')).ok())
            .ok_or_else(missing)?;
        let claims = crate::input::value(&payload).map_err(|_| missing())?;
        claims["sub"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(missing)
    }
    pub(crate) fn report(&self) -> Value {
        json!({"authenticated":self.headers.is_some(),"source":"NewBeeBox desktop auth-store","auth_store_source":"windows-known-folder",
        "api_origins":{"creator":"https://api.newbeebox.com","auth":"https://api.next.newbeebox.com","metadata":"https://cdn2.newbeebox.com"}})
    }
    #[cfg(test)]
    pub(crate) fn fixture(store: PathBuf) -> Self {
        Self {
            store,
            headers: None,
            desktop: None,
        }
    }
}
fn auth(message: &str) -> ToolkitError {
    ToolkitError::new("publish.newbee_auth", message, 3)
}
fn secret(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| auth("NewBeeBox authentication response is missing a required credential"))
}
fn data(value: Value) -> Result<Value> {
    if value["code"].as_i64() != Some(1) || !value["data"].is_object() {
        return Err(auth("NewBeeBox Creator authentication exchange failed"));
    }
    Ok(value["data"].clone())
}
fn unix_seconds() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_| auth("system clock is outside the authentication range"))
}
fn fresh(token: &str, now: u64) -> bool {
    token
        .split('.')
        .nth(1)
        .and_then(|part| URL_SAFE_NO_PAD.decode(part.trim_end_matches('=')).ok())
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|v| v["exp"].as_u64())
        .is_some_and(|expiry| expiry > now.saturating_add(30))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Reply, Server, TestDir};
    fn jwt(expiry: u64) -> String {
        format!(
            "Fixture.{}.Signature",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({"exp":expiry})).unwrap())
        )
    }
    fn credentials(dir: &TestDir, access: &str) {
        fs::write(dir.0.join("access-token"), access).unwrap();
        fs::write(dir.0.join("refresh-token"), "Refresh+&Secret").unwrap();
    }
    #[test]
    fn receipt_subject_is_stable_for_an_operation_and_changes_with_a_new_desktop_account() {
        let dir = TestDir::new();
        let expiry = unix_seconds().unwrap() + 600;
        let access = |subject: &str| {
            format!(
                "Fixture.{}.Signature",
                URL_SAFE_NO_PAD
                    .encode(serde_json::to_vec(&json!({"exp":expiry,"sub":subject})).unwrap())
            )
        };
        credentials(&dir, &access("author-A"));
        let server = Server::new(Vec::new());
        let control = OperationControl::default();
        let http = Http::loopback(&control, server.url.clone());
        let mut original = CreatorSession::fixture(dir.0.clone());
        assert_eq!(original.subject(&http, &control).unwrap(), "author-A");
        credentials(&dir, &access("author-B"));
        assert_eq!(original.subject(&http, &control).unwrap(), "author-A");
        let mut next = CreatorSession::fixture(dir.0.clone());
        assert_eq!(next.subject(&http, &control).unwrap(), "author-B");
        credentials(&dir, &jwt(expiry));
        let mut missing = CreatorSession::fixture(dir.0.clone());
        assert_eq!(missing.subject(&http, &control).unwrap_err().exit_code, 3);
        assert!(server.finish().is_empty());
    }
    pub(super) fn handshake() -> Vec<Reply> {
        vec![
            Reply::Json(json!({"code":1,"data":{"code":"one-time-code"}})),
            Reply::Json(
                json!({"code":1,"data":{"token":"CreatorSecret","jwtToken":"InitialSecret"}}),
            ),
            Reply::Json(json!({"code":1,"data":{"resource_token":"ResourceSecret"}})),
        ]
    }
    #[test]
    fn jwt_expiry_is_only_local_refresh_admission_with_fixed_leeway() {
        assert!(fresh(&jwt(131), 100));
        assert!(!fresh(&jwt(130), 100));
        assert!(!fresh(&jwt(99), 100));
        for token in [
            "",
            "not-a-jwt",
            "head.invalid.sig",
            "head.eyJleHAiOnRydWV9.sig",
        ] {
            assert!(!fresh(token, 100));
        }
    }
    #[test]
    fn current_desktop_access_is_exchanged_once_and_private_headers_are_cached() {
        let dir = TestDir::new();
        let access = jwt(unix_seconds().unwrap() + 600);
        credentials(&dir, &access);
        let server = Server::new(handshake());
        let control = OperationControl::default();
        let http = Http::loopback(&control, server.url.clone());
        let mut session = CreatorSession::fixture(dir.0.clone());
        let headers = session.headers(&http, &control).unwrap();
        assert_eq!(headers["authorization"], "Bearer ResourceSecret");
        assert_eq!(headers["token"], "CreatorSecret");
        assert!(headers["token"].is_sensitive());
        assert_eq!(session.headers(&http, &control).unwrap(), headers);
        let public = session.report().to_string();
        for secret in [
            &access,
            "Refresh+&Secret",
            "CreatorSecret",
            "ResourceSecret",
            "InitialSecret",
            "one-time-code",
        ] {
            assert!(!public.contains(secret));
        }
        let requests = server.finish();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].line, "POST /v3/user/auth2web HTTP/1.1");
        assert!(
            requests[0]
                .headers
                .contains(&format!("authorization: Bearer {access}"))
        );
        assert!(requests[1].headers.contains("appid: 6"));
        assert_eq!(
            serde_json::from_slice::<Value>(&requests[1].body).unwrap(),
            json!({"code":"one-time-code"})
        );
        assert!(requests[2].headers.contains("token: CreatorSecret"));
        assert!(
            requests[2]
                .headers
                .contains("authorization: Bearer InitialSecret")
        );
    }
    #[test]
    fn desktop_backup_headers_use_the_original_access_without_a_creator_exchange() {
        let dir = TestDir::new();
        let access = jwt(unix_seconds().unwrap() + 600);
        credentials(&dir, &access);
        let server = Server::new(vec![]);
        let control = OperationControl::default();
        let http = Http::loopback(&control, server.url.clone());
        let mut session = CreatorSession::fixture(dir.0.clone());
        let headers = session.desktop_headers(&http, &control).unwrap();
        assert_eq!(headers["authorization"], format!("Bearer {access}"));
        assert!(headers["authorization"].is_sensitive());
        assert!(!headers.contains_key("token"));
        assert!(!headers.contains_key("appid"));
        assert_eq!(session.desktop_headers(&http, &control).unwrap(), headers);
        assert!(server.finish().is_empty());
    }
    #[test]
    fn expired_access_uses_the_official_form_and_persists_rotated_desktop_credentials() {
        let dir = TestDir::new();
        credentials(&dir, &jwt(1));
        fs::write(dir.0.join("device-proof"), "proof+&secret").unwrap();
        let new_access = jwt(unix_seconds().unwrap() + 600);
        let mut replies = vec![Reply::Json(
            json!({"access_token":new_access,"refresh_token":"rotated-refresh","device_proof":"rotated-proof"}),
        )];
        replies.extend(handshake());
        let server = Server::new(replies);
        let control = OperationControl::default();
        let http = Http::loopback(&control, server.url.clone());
        let mut session = CreatorSession::fixture(dir.0.clone());
        session.headers(&http, &control).unwrap();
        assert_eq!(
            fs::read_to_string(dir.0.join("access-token")).unwrap(),
            new_access
        );
        assert_eq!(
            fs::read_to_string(dir.0.join("refresh-token")).unwrap(),
            "rotated-refresh"
        );
        assert_eq!(
            fs::read_to_string(dir.0.join("device-proof")).unwrap(),
            "rotated-proof"
        );
        let requests = server.finish();
        assert_eq!(requests[0].line, "POST /auth/connect/token HTTP/1.1");
        assert!(
            requests[0]
                .headers
                .contains("content-type: application/x-www-form-urlencoded")
        );
        let form = reqwest::Url::parse(&format!(
            "http://127.0.0.1/?{}",
            String::from_utf8_lossy(&requests[0].body)
        ))
        .unwrap();
        let fields = form
            .query_pairs()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(fields["client_id"], "nbb-desktop");
        assert_eq!(fields["grant_type"], "refresh_token");
        assert_eq!(fields["refresh_token"], "Refresh+&Secret");
        assert_eq!(fields["device_proof"], "proof+&secret");
        assert!(requests[1].headers.contains(&new_access));
    }
    #[test]
    fn authentication_business_failure_never_returns_the_echoed_credential_payload() {
        let dir = TestDir::new();
        credentials(&dir, &jwt(unix_seconds().unwrap() + 600));
        let server = Server::new(vec![Reply::Json(
            json!({"code":0,"data":{"token":"EchoedSecret"},"message":"Bearer EchoedSecret"}),
        )]);
        let control = OperationControl::default();
        let http = Http::loopback(&control, server.url.clone());
        let mut session = CreatorSession::fixture(dir.0.clone());
        let error = session.headers(&http, &control).unwrap_err();
        assert_eq!(error.code, "publish.newbee_auth");
        assert!(
            !serde_json::to_string(&error)
                .unwrap()
                .contains("EchoedSecret")
        );
        assert_eq!(server.finish().len(), 1);
    }
}
