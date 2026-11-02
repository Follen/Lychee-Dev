//! The optional channel operation keeps GUI credentials local to its reader.
use crate::transport::{Http, Origin, secret_header};
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::Value;
mod elevated;
mod gui;
mod jwt;
mod managed;
mod projection;

pub fn helper_entry(args: &[String]) -> Result<()> {
    elevated::entry(args)
}

pub(super) fn desktop(
    home: &std::path::Path,
    desktop: &super::desktop::Desktop,
    control: &OperationControl,
) -> Result<Value> {
    let anchor = desktop.channel_anchor();
    let choices = match gui::read(&anchor, control, || Ok(())) {
        Err(error) if error.code == gui::ACCESS_DENIED => elevated::call(home, &anchor, control),
        result => result,
    }?;
    anchor.verify(control)?;
    choices.value()
}

// These are the only values allowed across the optional elevation boundary.
// In particular, no raw HTTP body, JWT, login token or managed address is sent.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Choices {
    total: usize,
    items: Vec<Choice>,
    dependencies: [Dependency; 1],
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    room_id: String,
    room_name: Option<String>,
    channel_id: String,
    channel_name: Option<String>,
    channel_type: String,
}
#[derive(Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Dependency {
    parent: String,
    children: [String; 2],
}
fn dependency() -> [Dependency; 1] {
    [Dependency {
        parent: "room_id".into(),
        children: ["channel_id".into(), "channel_type".into()],
    }]
}
impl Choices {
    fn validate(&self) -> Result<()> {
        if self.total != self.items.len() || self.total > 10000 || self.dependencies != dependency()
        {
            return Err(invalid());
        }
        for item in &self.items {
            if item.room_id.is_empty()
                || [&item.room_id, &item.channel_id, &item.channel_type]
                    .into_iter()
                    .chain(item.room_name.iter())
                    .chain(item.channel_name.iter())
                    .any(|s| s.chars().count() > 4096)
            {
                return Err(invalid());
            }
        }
        if serde_json::to_vec(self)?.len() > 1 << 20 {
            return Err(invalid());
        }
        Ok(())
    }
    pub(super) fn value(self) -> Result<Value> {
        self.validate()?;
        Ok(serde_json::to_value(self)?)
    }
}
fn fault(message: &str) -> ToolkitError {
    ToolkitError::new("publish.dd_gui_channel", message, 3)
}
fn invalid() -> ToolkitError {
    ToolkitError::new(
        "publish.dd_channels",
        "DD channel response does not match the supported choices contract",
        5,
    )
}
fn accepted(value: &Value) -> bool {
    value
        .get("code")
        .is_some_and(|v| v.as_str() == Some("OK") || v.as_u64().is_some_and(|c| c == 0 || c == 200))
}
fn headers(secret: &str) -> Result<HeaderMap> {
    let mut headers = secret_header("authentication", secret)?;
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(super::USER_AGENT_VALUE),
    );
    Ok(headers)
}
fn query(http: &Http, token: &str) -> Result<Choices> {
    let value = http.get(
        Origin::DdCcApi,
        "/v1/mixteammsgproxy/channelList",
        &[("source", "pluginPublish".into())],
        headers(token)?,
    )?;
    if !accepted(&value) {
        return Err(invalid());
    }
    let choices = projection::choices(&value)?;
    reject_echo(&choices, &[token])?;
    Ok(choices)
}
fn reject_echo(choices: &Choices, secrets: &[&str]) -> Result<()> {
    choices.validate()?;
    // Check unescaped text too: JSON escaping must not hide an echoed token.
    if choices.items.iter().any(|item| {
        [&item.room_id, &item.channel_id, &item.channel_type]
            .into_iter()
            .chain(item.room_name.iter())
            .chain(item.channel_name.iter())
            .any(|text| {
                secrets
                    .iter()
                    .any(|secret| !secret.is_empty() && text.contains(secret))
            })
    }) {
        return Err(invalid());
    }
    Ok(())
}
pub(super) fn native(cc: &mut super::cc::CcSession, control: &OperationControl) -> Result<Value> {
    let http = Http::new(control)?;
    native_with(cc, &http, control)
}
pub(super) fn native_with(
    cc: &mut super::cc::CcSession,
    http: &Http,
    control: &OperationControl,
) -> Result<Value> {
    let token = cc.jwt_with(http, control)?;
    cc.check_active()?;
    let choices = query(http, &token)?;
    cc.check_active()?;
    control.check()?;
    choices.value()
}
fn gui_with<M: managed::Memory>(
    memory: &mut M,
    base: u64,
    expected_ccid: &str,
    http: &Http,
    control: &OperationControl,
    mut verify: impl FnMut() -> Result<()>,
    mut now: impl FnMut() -> f64,
) -> Result<Choices> {
    control.check()?;
    verify()?;
    let mut reader = managed::Reader::new(memory, control);
    let snapshot = reader.snapshot(base)?;
    if snapshot.ccid != expected_ccid {
        return Err(fault(
            "the original DD account profile does not match its GUI session",
        ));
    }
    verify()?;
    reader.verify(&snapshot)?;
    let token = jwt::current(&snapshot, http, now())?;
    control.check()?;
    reader.verify(&snapshot)?;
    verify()?;
    // Recheck the token after the exchange; a slow response cannot extend it.
    jwt::validate(&token, &snapshot, now()).and_then(|valid| {
        if valid {
            Ok(())
        } else {
            Err(fault("DD GUI channel JWT expired during its exchange"))
        }
    })?;
    let choices = query(http, &token)?;
    reader.verify(&snapshot)?;
    verify()?;
    control.check()?;
    reject_echo(&choices, &[&token, &snapshot.cached, &snapshot.login_token])?;
    Ok(choices)
}

#[cfg(test)]
mod tests;
