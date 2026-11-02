use super::{auth::Session, *};
use crate::transport::{Http, Origin, secret_header};
use reqwest::header::HeaderMap;

pub(super) struct Api {
    pub(super) http: Http,
    pub(super) session: Option<Session>,
}
impl Api {
    pub(super) fn write(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
        build: Option<u8>,
    ) -> Result<Value> {
        checked(self.http.json_request(
            method,
            Origin::Modus,
            &format!("/api/{path}"),
            query,
            self.headers(build)?,
            body,
            true,
        )?)
    }
    pub(super) fn headers(&self, build: Option<u8>) -> Result<HeaderMap> {
        let s = self
            .session
            .as_ref()
            .ok_or_else(|| malformed("ModUs operation requires its declared client identity"))?;
        let mut headers = secret_header("authorization", &s.authorization())?;
        if let Some(device) = &s.device {
            headers.extend(secret_header("x-device-id", device)?);
        }
        if let Some(build) = build {
            headers.insert(
                "x-server-type",
                build.to_string().parse().expect("build header"),
            );
        }
        Ok(headers)
    }
    pub(super) fn build(&self, explicit: Option<u8>) -> Result<u8> {
        self.session
            .as_ref()
            .ok_or_else(|| malformed("ModUs main identity is required"))?
            .build(explicit)
    }
    pub(super) fn get(
        &self,
        path: &str,
        query: &[(&str, String)],
        build: Option<u8>,
    ) -> Result<Value> {
        checked(self.http.get(
            Origin::Modus,
            &format!("/api/{path}"),
            query,
            self.headers(build)?,
        )?)
    }
    pub(super) fn post(
        &self,
        path: &str,
        body: &Value,
        build: Option<u8>,
        write: bool,
    ) -> Result<Value> {
        checked(self.http.post(
            Origin::Modus,
            &format!("/api/{path}"),
            self.headers(build)?,
            body,
            write,
        )?)
    }
    pub(super) fn account(&self) -> Result<String> {
        let value = data(self.get("system/user/getInfo", &[], None)?);
        let user = value.get("user").unwrap_or(&value);
        identity(
            user.get("userId")
                .or_else(|| user.get("id"))
                .unwrap_or(&Value::Null),
        )
        .ok_or_else(|| malformed("ModUs account response omitted its identity"))
    }
    pub(super) fn backup_detail(&self, id: &str, build: u8) -> Result<Option<Value>> {
        let value = match self.http.get(
            Origin::Modus,
            &format!("/api/system/user/backup/detail/{}", segment(id)),
            &[],
            self.headers(Some(build))?,
        ) {
            Ok(v) => v,
            Err(e) if e.code == "publish.http_status" && e.details["http_status"] == 404 => {
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        if value["code"] == 404 || value["code"] == 500 && value["msg"] == "没有备份信息，获取失败"
        {
            return Ok(None);
        }
        checked(value).map(Some)
    }
    pub(super) fn static_options(&self, file: &str) -> Result<Value> {
        let value = checked(self.http.get(
            Origin::ModusStatic,
            &format!("/modus/client_static_api/{file}"),
            &[],
            HeaderMap::new(),
        )?)?;
        let value = data(value);
        Ok(value.get("rows").cloned().unwrap_or(value))
    }
}
pub(super) fn checked(value: Value) -> Result<Value> {
    let code = value.get("code").and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    });
    if value.get("success") == Some(&json!(false)) || code.is_some_and(|n| n >= 400) {
        return Err(ToolkitError::new("publish.modus_rejected","ModUs rejected the operation; check account permissions and the current resource options",5).with_details(json!({"business_code":code,"submission":"rejected","replay_allowed":false})));
    }
    Ok(value)
}
pub(super) fn data(value: Value) -> Value {
    value.get("data").cloned().unwrap_or(value)
}
pub(super) fn identity(v: &Value) -> Option<String> {
    match v {
        Value::String(s)
            if !s.is_empty()
                && s.len() <= 256
                && !s.chars().any(|c| c.is_whitespace() || c.is_control())
                && s != "0" =>
        {
            Some(s.clone())
        }
        Value::Number(n) if n.as_u64().is_some_and(|n| n > 0) => Some(n.to_string()),
        _ => None,
    }
}
pub(super) fn segment(value: &str) -> String {
    let mut url = reqwest::Url::parse("https://app.modus.cool/").unwrap();
    url.path_segments_mut().unwrap().push(value);
    url.path().trim_start_matches('/').into()
}
