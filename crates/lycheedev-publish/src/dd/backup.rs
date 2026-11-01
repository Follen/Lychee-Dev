//! DD backups: one catalog/identity boundary for publication and management.
use super::{Reference, forms, read};
use crate::input::invalid;
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use serde_json::{Value, json};
use std::collections::BTreeSet;
mod bundle;
mod cloud;
pub(super) mod upload;
pub use bundle::{BackupGenerate, scan};
pub use cloud::{BackupDelete, BackupRename, BackupSelection};
pub use upload::{BackupCreate, BackupUpload};
fn data(message: &str) -> ToolkitError {
    ToolkitError::new("publish.dd_backup", message, 5)
}
pub(super) fn reference(value: &Value) -> Result<String> {
    let mut selected = None;
    for key in ["sn", "backup_sn"] {
        if let Some(v) = value
            .get(key)
            .filter(|v| !v.is_null() && v.as_str() != Some(""))
        {
            let s =
                forms::scalar(v).ok_or_else(|| data("DD backup identity has an invalid type"))?;
            Reference::parse(&s).map_err(|_| data("DD backup identity is invalid"))?;
            if selected.as_ref().is_some_and(|old| old != &s) {
                return Err(data("DD backup detail has contradictory SNs"));
            }
            selected = Some(s);
        }
    }
    selected.ok_or_else(|| data("DD backup omitted its original identity"))
}
pub(super) fn list(api: &mut impl read::Api) -> Result<Vec<Value>> {
    let payload = api.get("/backup/list", &[])?;
    let rows = read::items(&payload)?;
    if read::total(&payload)?.is_some_and(|n| n != rows.len() as u64) {
        return Err(data(
            "DD backup membership requires the complete current list",
        ));
    }
    let mut seen = BTreeSet::new();
    for row in rows {
        if !seen.insert(reference(row)?) {
            return Err(data("DD backup list repeats an identity"));
        }
    }
    Ok(rows.to_vec())
}
pub(super) fn detail(api: &mut impl read::Api, sn: &str) -> Result<Value> {
    let payload = api.get("/backup/detail", &[("sn", sn.into())])?;
    let v = payload
        .get("result")
        .ok_or_else(|| data("DD backup omitted its result"))?;
    let v = if let Some(a) = v.as_array() {
        if a.len() != 1 {
            return Err(data("DD backup detail is not one selected backup"));
        }
        &a[0]
    } else {
        v
    };
    if !v.is_object() || reference(v)? != sn {
        return Err(data("DD backup detail differs from the exact selected SN"));
    }
    read::backup_view(v, sn)?;
    Ok(v.clone())
}
pub(super) fn selected(api: &mut impl read::Api, sn: &str) -> Result<Value> {
    for row in list(api)? {
        if reference(&row)? == sn {
            return detail(api, sn);
        }
    }
    Err(invalid(
        "$.backup_sn",
        "backup is absent from the retained author's complete current list",
    ))
}
fn author(client: &super::task::Client, control: &OperationControl) -> Result<String> {
    let v = client.author_identity(control)?;
    v.pointer("/author/author_id")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty() && s.len() <= 1024 && !s.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or_else(|| data("retained DD task omitted its original author identity"))
}
fn target(author: &str, sn: &str) -> Result<u64> {
    let hash = lycheedev_core::storage::sha256(&lycheedev_core::storage::identity_json(
        &json!({"author":author,"resource":"backup","sn":sn}),
    )?);
    Ok(u64::from_str_radix(&hash[..16], 16).unwrap().max(1))
}
