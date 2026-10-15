//! Fully authenticated CASC bytes or one bounded, stable local input. No decoder
//! clones the retained body; frame demux borrows slices from this single buffer.
use super::{fault, output};
use crate::{
    local_query,
    resource::{Budget, Cost},
};
use lycheedev_core::{
    Invocation, Result, ToolkitError,
    evidence::{self, CaptureInput},
    selection, storage,
};
use serde_json::{Value, json};
use std::{fs, io::Read, path::Path, sync::Arc};

pub struct CascInput {
    pub bytes: local_query::FileBytes,
    pub reading: Value,
    pub capture: Value,
    pub snapshot: String,
    pub data_build: String,
}
pub struct VerifiedCascInput {
    pub blob: evidence::VerifiedBlob,
    pub reading: Value,
    pub capture: Value,
    pub snapshot: String,
    pub data_build: String,
    pub budget: Arc<Budget>,
}
pub fn capture_casc_verified(inv: &Invocation) -> Result<VerifiedCascInput> {
    inv.control.check()?;
    let pin = selection::resolve_pinned(inv)?;
    let data = pin.data.as_ref().ok_or_else(|| {
        ToolkitError::new("records.data_pin_required", "records.data_pin_required", 2)
    })?;
    let id = inv.required("--file-id")?.parse::<u32>()
        .map_err(|_| ToolkitError::arguments("invalid --file-id"))?;
    let budget = Arc::new(Budget::controlled(Cost::defaults(), inv.control.deadline(), inv.control.clone()));
    let file = local_query::read_verified_file_budgeted(inv, data, id, &budget)?;
    let capture = evidence::commit_verified_capture_budgeted(
        &file.blob,
        evidence::CaptureHeader {
            media_type: "application/octet-stream",
            provenance: json!({"kind":"asset","locator":format!("casc:fdid:{id}:ckey:{}",file.reading["entry"]["contentKey"].as_str().unwrap_or("")),"snapshot":pin.id,"dataBuild":data.full_build}),
            complete: true, truncated: false,
        },
        &inv.control, budget.as_ref(),
    )?;
    Ok(VerifiedCascInput {
        blob: file.blob, reading: file.reading, capture,
        snapshot: pin.id.clone(), data_build: data.full_build.clone(), budget,
    })
}
pub fn capture_casc(inv: &Invocation, maximum: usize) -> Result<CascInput> {
    inv.control.check()?;
    let pin = selection::resolve_pinned(inv)?;
    let data = pin.data.as_ref().ok_or_else(|| {
        ToolkitError::new("records.data_pin_required", "records.data_pin_required", 2)
    })?;
    let id = inv
        .required("--file-id")?
        .parse::<u32>()
        .map_err(|_| ToolkitError::arguments("invalid --file-id"))?;
    if id == 0 {
        return Err(ToolkitError::arguments("invalid --file-id"));
    }
    let budget = Arc::new(Budget::controlled(
        Cost::defaults(),
        inv.control.deadline(),
        inv.control.clone(),
    ));
    let file = local_query::read_files_budgeted(inv, data, &[id], &budget)?.remove(0);
    if file.bytes.len() > maximum {
        return Err(fault("records.metadata_limit", 3));
    }
    budget.check()?;
    let capture = evidence::commit_capture_budgeted(
        &inv.home,
        CaptureInput {
            bytes: &file.bytes,
            maximum_bytes: maximum,
            media_type: "application/octet-stream",
            provenance: json!({"kind":"asset","locator":format!("casc:fdid:{id}:ckey:{}",file.reading["entry"]["contentKey"].as_str().unwrap_or("")),"snapshot":pin.id,"dataBuild":data.full_build}),
            complete: true,
            truncated: false,
        },
        &inv.control,
        budget.as_ref(),
    )?;
    Ok(CascInput {
        bytes: file.bytes,
        reading: file.reading,
        capture,
        snapshot: pin.id.clone(),
        data_build: data.full_build.clone(),
    })
}
pub fn local(inv: &Invocation, path: &Path, maximum: usize) -> Result<(Vec<u8>, String)> {
    inv.control.check()?;
    let canonical = output::ordinary(&fs::canonicalize(path)?);
    let parent = canonical
        .parent()
        .ok_or_else(|| fault("records.file_query", 2))?;
    let directory = storage::Directory::open(parent)?;
    let name = canonical
        .file_name()
        .ok_or_else(|| fault("records.file_query", 2))?;
    let info = directory.metadata(name)?;
    if !info.is_file() {
        return Err(fault("records.file_query", 2));
    }
    if info.len() > maximum as u64 {
        return Err(fault("records.demux_input_limit", 3));
    }
    let mut file = directory.open_file(name)?;
    let current = file.metadata()?;
    if !current.is_file() || current.len() != info.len() {
        return Err(fault("records.file_query", 2));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(info.len() as usize)
        .map_err(|_| fault("records.demux_input_limit", 3))?;
    let mut buffer = [0u8; 65536];
    loop {
        inv.control.check()?;
        let remaining = maximum.saturating_sub(bytes.len()).saturating_add(1);
        let requested = remaining.min(buffer.len());
        let n = file.read(&mut buffer[..requested])?;
        inv.control.check()?;
        if n == 0 {
            break;
        }
        if n > maximum - bytes.len() {
            return Err(fault("records.demux_input_limit", 3));
        }
        bytes.extend_from_slice(&buffer[..n]);
    }
    if bytes.len() as u64 != info.len() {
        return Err(fault("records.file_query", 2));
    }
    Ok((bytes, canonical.to_string_lossy().into_owned()))
}
