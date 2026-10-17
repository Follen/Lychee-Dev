//! Fully authenticated CASC bytes or one bounded, stable local input. No decoder
//! clones the retained body; frame demux borrows slices from this single buffer.
use super::{fault, output};
use crate::{
    local_query,
    phase_profile::Phase,
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
/// The selected source cannot outlive its query or real workspace admission.
/// Only the immutable body is mutably borrowed for Read/rewind, never detached.
pub(crate) struct VerifiedView<'a> {
    pub metadata: &'a mut lycheedev_core::metadata::Metadata,
    pub blob: &'a mut evidence::VerifiedBlob,
    pub reading: &'a Value,
    pub capture: &'a Value,
    pub snapshot: &'a str,
    pub data_build: &'a str,
    pub budget: &'a Arc<Budget>,
}
pub(crate) fn with_casc_verified<R>(
    inv: &Invocation,
    id: u32,
    callback: &mut dyn FnMut(VerifiedView<'_>) -> Result<R>,
) -> Result<R> {
    inv.control.check()?;
    // Deferred Go323 query budget: sampling belongs to actual admission, not
    // this constructor. The Arc/address remains the original query identity.
    let budget = Arc::new(Budget::query_controlled(
        inv.control.deadline(),
        inv.control.clone(),
    ));
    with_casc_verified_budgeted(inv, id, &budget, callback)
}
// Explicit budgets let focused regressions observe the actual consumer without
// process-global environment mutation or replacing any source/capture operation.
pub(super) fn with_casc_verified_budgeted<R>(
    inv: &Invocation,
    id: u32,
    budget: &Arc<Budget>,
    callback: &mut dyn FnMut(VerifiedView<'_>) -> Result<R>,
) -> Result<R> {
    with_casc_verified_budgeted_finish(inv, id, budget, callback, |scope, result| scope.finish(result))
}
// Observe only after the real SQLite close. Tests can add a late diagnostic,
// never replace the real close result or skip any production cleanup.
#[cfg(test)]
pub(super) fn with_casc_verified_budgeted_after_metadata_close<R>(
    inv: &Invocation,
    id: u32,
    budget: &Arc<Budget>,
    callback: &mut dyn FnMut(VerifiedView<'_>) -> Result<R>,
    after_close: &mut dyn FnMut() -> Result<()>,
) -> Result<R> {
    with_casc_verified_budgeted_finish(inv, id, budget, callback, |scope, result| {
        scope.finish_after_metadata_close(result, after_close)
    })
}
fn with_casc_verified_budgeted_finish<'a, R>(
    inv: &'a Invocation,
    id: u32,
    budget: &'a Arc<Budget>,
    callback: &mut dyn FnMut(VerifiedView<'_>) -> Result<R>,
    finish: impl FnOnce(crate::remote::provider::Scope<'a>, Result<R>) -> Result<R>,
) -> Result<R> {
    let (mut scope, pin) = {
        let _phase = budget.phase(Phase::Admission);
        let opened = crate::remote::provider::OpenedScope::open(inv, budget)?;
        opened.admit_asset()?
    };
    let data = pin.data.as_ref().expect("asset pin admitted");
    let result = scope.with_capture_metadata(|scope, metadata| {
        let mut consume = |blob: &mut evidence::VerifiedBlob, reading: &Value| {
            budget.charge_split(
                Cost::retained(
                    blob.reference()
                        .bytes
                        .try_into()
                        .map_err(|_| fault("records.metadata_limit", 3))?,
                ),
                Cost::default(),
            )?;
            let capture_phase = budget.phase(Phase::SourceCapture);
            let capture = evidence::commit_verified_capture_budgeted_with_metadata(
                blob,
                metadata,
                evidence::CaptureHeader {
                    media_type: "application/octet-stream",
                    provenance: json!({"kind":"asset","locator":format!("casc:fdid:{id}:ckey:{}",reading["entry"]["contentKey"].as_str().unwrap_or("")),"snapshot":pin.id,"dataBuild":data.full_build}),
                    complete: true,
                    truncated: false,
                },
                &inv.control,
                &budget.safety_only(),
            )?;
            drop(capture_phase);
            callback(VerifiedView {
                metadata: &mut *metadata,
                blob,
                reading,
                capture: &capture,
                snapshot: &pin.id,
                data_build: &data.full_build,
                budget,
            })
        };
        (|| {
            if inv.flag_bool("--cdn") {
                scope.with_verified_file(data, id, &mut consume)
            } else {
                let mut reader = local_query::sql_reader::Reader::new(inv, data, budget)?;
                let result = (|| {
                    let mut file = reader.read_verified(id)?;
                    let result = (|| {
                        reader.verify_source()?;
                        consume(&mut file.blob, &file.reading)
                    })();
                    let _phase = budget.phase(Phase::SourceClose);
                    file.blob.close_after(result)
                })();
                reader.close_after(result)
            }
        })()
    });
    let _phase = budget.phase(Phase::SourceClose);
    finish(scope, result)
}
pub(crate) fn with_casc_bytes<R>(
    inv: &Invocation,
    id: u32,
    maximum: usize,
    callback: &mut dyn FnMut(CascInput, &Arc<Budget>) -> Result<R>,
) -> Result<R> {
    inv.control.check()?;
    let budget = Arc::new(Budget::query_controlled(
        inv.control.deadline(),
        inv.control.clone(),
    ));
    let (mut scope, pin) =
        crate::remote::provider::OpenedScope::open(inv, &budget)?.admit_asset()?;
    let data = pin.data.as_ref().expect("asset pin admitted");
    let mut consume = |file: local_query::File| {
        if file.bytes.len() > maximum {
            return Err(fault("records.demux_input_limit", 3));
        }
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
            &budget.safety_only(),
        )?;
        callback(
            CascInput {
                bytes: file.bytes,
                reading: file.reading,
                capture,
                snapshot: pin.id.clone(),
                data_build: data.full_build.clone(),
            },
            &budget,
        )
    };
    let result = (|| {
        if inv.flag_bool("--cdn") {
            let mut result = None;
            let read = scope.read_batches(data, &[id], &mut |mut files| {
                let file = files.pop().ok_or_else(|| fault("records.internal", 8))?;
                result = Some(consume(file)?);
                Ok(Vec::new())
            });
            read.and_then(|()| result.ok_or_else(|| fault("records.internal", 8)))
        } else {
            let mut reader = local_query::sql_reader::Reader::new(inv, data, &budget)?;
            let result = (|| {
                let file = reader.read(id)?;
                reader.verify_source()?;
                consume(file)
            })();
            reader.close_after(result)
        }
    })();
    scope.finish(result)
}

pub fn capture_casc_verified(inv: &Invocation) -> Result<VerifiedCascInput> {
    inv.control.check()?;
    let pin = selection::resolve_pinned(inv)?;
    let data = pin.data.as_ref().ok_or_else(|| {
        ToolkitError::new("records.data_pin_required", "records.data_pin_required", 2)
    })?;
    let id = inv
        .required("--file-id")?
        .parse::<u32>()
        .map_err(|_| ToolkitError::arguments("invalid --file-id"))?;
    let budget = Arc::new(Budget::controlled(
        Cost::defaults(),
        inv.control.deadline(),
        inv.control.clone(),
    ));
    let file = local_query::read_verified_file_budgeted(inv, data, id, &budget)?;
    let capture = evidence::commit_verified_capture_budgeted(
        &file.blob,
        evidence::CaptureHeader {
            media_type: "application/octet-stream",
            provenance: json!({"kind":"asset","locator":format!("casc:fdid:{id}:ckey:{}",file.reading["entry"]["contentKey"].as_str().unwrap_or("")),"snapshot":pin.id,"dataBuild":data.full_build}),
            complete: true,
            truncated: false,
        },
        &inv.control,
        budget.as_ref(),
    )?;
    Ok(VerifiedCascInput {
        blob: file.blob,
        reading: file.reading,
        capture,
        snapshot: pin.id.clone(),
        data_build: data.full_build.clone(),
        budget,
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
    struct Check<'a>(&'a lycheedev_core::control::OperationControl);
    impl storage::FileControl for Check<'_> {
        type Error = ToolkitError;
        fn check(&mut self) -> Result<()> {
            self.0.check()
        }
        fn charge(&mut self, _: storage::FileCost) -> Result<()> {
            self.0.check()
        }
    }
    let mut check = Check(&inv.control);
    let directory = storage::Directory::open_controlled(parent, &mut check)?;
    let result = (|| {
        let name = canonical
            .file_name()
            .ok_or_else(|| fault("records.file_query", 2))?;
        let info = directory.metadata_controlled(name, &mut check)?;
        if !info.is_file() {
            return Err(fault("records.file_query", 2));
        }
        if info.len() > maximum as u64 {
            return Err(fault("records.demux_input_limit", 3));
        }
        let mut file = directory.open_file_controlled(name, &mut check)?;
        let read = (|| {
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
        })();
        storage::finish_file(file, read, &inv.control)
    })();
    output::finish_closes(result, directory.close(), &inv.control)
}
