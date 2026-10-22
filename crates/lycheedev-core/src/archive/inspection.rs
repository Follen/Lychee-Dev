//! Doctor's read-only journal boundary. It does not prepare or resume a switch.
use super::*;
use crate::control::OperationControl;
use serde::Deserialize;
use std::{fmt, io::Read};
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Journal {
    pub state: String,
    #[serde(default)]
    pub archive_path: String,
    schema: String,
    root: String,
    journal_path: String,
    created_at: String,
    #[serde(default)]
    completed_at: Option<String>,
    #[serde(default)]
    markers: Vec<String>,
    steps: Vec<Step>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Step { id: String, detail: String }
fn invalid(detail: impl fmt::Display) -> ToolkitError {
    ToolkitError::new(
        "vault.archive_state",
        format!("vault.archive_state: {detail}"),
        4,
    )
}
fn decode(raw: &[u8], root: &Path, path: &Path, control: &OperationControl) -> Result<Journal> {
    let value: Journal = serde_json::from_slice(raw).map_err(|e| invalid(format!("archive journal format: {e}")))?;
    if storage::parse_timestamp(&value.created_at).is_none() || value.completed_at.as_deref().is_some_and(|time| storage::parse_timestamp(time).is_none()) { return Err(invalid("invalid archive timestamp")); }
    if value.schema != "lycheedev.archive.v1"
        || value.root != root.to_string_lossy()
        || value.journal_path != path.to_string_lossy()
    {
        return Err(invalid("archive journal identity"));
    }
    if !["planned", "moved", "complete"].contains(&value.state.as_str()) {
        return Err(invalid(format!("archive journal state {:?}", value.state)));
    }
    control.check()?;
    Ok(value)
}
pub(crate) fn journal_controlled(
    root: &Path,
    control: &OperationControl,
) -> Result<Option<Journal>> {
    control.check()?;
    let root = absolute(root)?;
    let path = journal_path(&root)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            control.check()?;
            return Ok(None);
        }
        Err(e) => return Err(e.into()),
    };
    control.check()?;
    if !metadata.is_file() || metadata.len() > 65536 {
        return Err(invalid("archive journal shape"));
    }
    storage::guard_path_with_check(path.parent().unwrap(), &path, || control.check())?;
    let mut file = match fs::File::open(&path) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            control.check()?;
            return Ok(None);
        }
        Err(e) => return Err(e.into()),
    };
    let result = (|| {
        let mut raw = Vec::with_capacity(metadata.len() as usize);
        let mut buffer = [0; 8192];
        while raw.len() < 65537 {
            control.check()?;
            let request = buffer.len().min(65537 - raw.len());
            let n = file.read(&mut buffer[..request])?;
            raw.extend_from_slice(&buffer[..n]);
            control.check()?;
            if n == 0 {
                break;
            }
        }
        if raw.len() > 65536 {
            return Err(invalid("archive journal encoding"));
        }
        decode(&raw, &root, &path, control)
    })();
    storage::finish_file(file, result, control).map(Some)
}
#[cfg(test)]
mod tests;
