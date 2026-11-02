//! Validate only the current GUI identity; this is not JWT signature verification.
use super::{accepted, fault, headers, managed::Snapshot};
use crate::{
    input,
    transport::{Http, Origin},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use lycheedev_core::Result;
use md5::{Digest, Md5};
use serde_json::Value;

pub(super) fn validate(token: &str, snapshot: &Snapshot, now: f64) -> Result<bool> {
    let bad = || fault("DD GUI channel JWT does not match its current identity or expiry contract");
    if token.is_empty() || token.len() > 16384 || !now.is_finite() || now <= 0.0 {
        return Err(bad());
    }
    let parts = token.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
    {
        return Err(bad());
    }
    let decode = |part| -> Result<Value> {
        let bytes = URL_SAFE_NO_PAD.decode(part).map_err(|_| bad())?;
        input::value(&bytes).map_err(|_| bad())
    };
    let header = decode(parts[0])?;
    let claims = decode(parts[1])?;
    if !header
        .get("alg")
        .and_then(Value::as_str)
        .is_some_and(|alg| !alg.is_empty() && !alg.eq_ignore_ascii_case("none"))
        || claims.get("appid").and_then(Value::as_str) != Some("webcc")
    {
        return Err(bad());
    }
    let user = match claims.get("user") {
        Some(Value::String(value)) => input::value(value.as_bytes()).map_err(|_| bad())?,
        Some(value @ Value::Object(_)) => value.clone(),
        _ => return Err(bad()),
    };
    for (key, expected) in [("uid", &snapshot.uid), ("eid", &snapshot.eid)] {
        let actual = match user.get(key) {
            Some(Value::String(value)) => value.clone(),
            Some(Value::Number(value)) => value.to_string(),
            _ => return Err(bad()),
        };
        if actual != *expected {
            return Err(bad());
        }
    }
    let expiry = claims
        .get("exp")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v >= 0.0 && *v <= now + 1200.0)
        .ok_or_else(bad)?;
    Ok(expiry > now + 5.0)
}
pub(super) fn current(snapshot: &Snapshot, http: &Http, now: f64) -> Result<String> {
    if !snapshot.cached.is_empty() && validate(&snapshot.cached, snapshot, now)? {
        return Ok(snapshot.cached.clone());
    }
    let timestamp = (now + snapshot.delta).trunc();
    if !timestamp.is_finite() || timestamp <= 0.0 || timestamp >= i64::MAX as f64 {
        return Err(fault("DD GUI server time is outside its supported range"));
    }
    let timestamp = (timestamp as i64).to_string();
    let signature = format!(
        "{:x}",
        Md5::digest(format!("webcc_{timestamp}_{}", snapshot.login_token).as_bytes())
    );
    let value = http.get(
        Origin::DdCcApi,
        "/v1/jwt",
        &[
            ("appid", "webcc".into()),
            ("uid", snapshot.uid.clone()),
            ("eid", snapshot.eid.clone()),
            ("timestamp", timestamp),
        ],
        headers(&signature)?,
    )?;
    let token = value
        .pointer("/data/jwt")
        .and_then(Value::as_str)
        .filter(|_| accepted(&value))
        .ok_or_else(|| fault("DD rejected the existing GUI login-token JWT exchange"))?;
    if !validate(token, snapshot, now)? {
        return Err(fault("DD returned an expired GUI channel JWT"));
    }
    Ok(token.into())
}
