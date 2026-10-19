//! Existing evidence owners under their original invocation and supplemental
//! scope. No replacement deadline or accounting ledger is introduced.
use super::*;
use lycheedev_windows::filesystem::destination::{
    FileControl, FileCost, StagedFile, StagedPublication,
};
pub(in crate::evidence) struct PathControl<'a> {
    pub control: Option<&'a OperationControl>,
    pub supplemental: &'a mut dyn FnMut() -> Result<()>,
}
impl FileControl for PathControl<'_> {
    type Error = ToolkitError;
    fn charge(&mut self, _: FileCost) -> Result<()> {
        self.check()
    }
    fn check(&mut self) -> Result<()> {
        check(self.control)?;
        (self.supplemental)()
    }
}
pub(in crate::evidence) fn finish_controls<T>(
    result: Result<T>,
    control: Option<&OperationControl>,
    supplemental: &mut impl FnMut() -> Result<()>,
) -> Result<T> {
    let result = finish_check(result, check(control));
    finish_check(result, supplemental())
}
pub(in crate::evidence) fn open_workspace(
    home: &Path,
    control: Option<&OperationControl>,
    supplemental: &mut impl FnMut() -> Result<()>,
) -> Result<Value> {
    match control {
        Some(control) => workspace::open_controlled_with_check(home, control, supplemental),
        None => {
            supplemental()?;
            workspace::open(home)
        }
    }
}
pub(super) fn close_blob<T>(
    opened: OpenBlob,
    result: Result<T>,
    control: Option<&OperationControl>,
    supplemental: &mut impl FnMut() -> Result<()>,
) -> Result<T> {
    let result = crate::metadata::historical_write::finish_historical(
        result,
        storage::close_file(opened.file).map_err(Into::into),
    );
    let result = optional::finish_directory(result, opened._directory);
    finish_controls(result, control, supplemental)
}
/// A streaming producer validates its digest/terminal EOF; prehashed immutable
/// bytes take the separate borrowed-chunk path without a second hash or buffer.
pub(super) fn publish_known(
    home: &Path,
    path: &Path,
    reader: &mut impl Read,
    reference: &BlobRef,
    maximum: u64,
    control: Option<&OperationControl>,
    supplemental: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    publish_stage(
        home,
        path,
        reference,
        maximum,
        control,
        supplemental,
        &mut BlobPublicationSettlement::new(),
        |stage, supplemental| {
            let mut remaining = reference.bytes;
            let mut buffer = [0u8; BLOCK];
            while remaining != 0 {
                check_with(control, supplemental)?;
                let size = remaining.min(BLOCK as u64) as usize;
                let n = finish_controls(
                    reader.read(&mut buffer[..size]).map_err(io_error),
                    control,
                    supplemental,
                )?;
                if n == 0 {
                    return Err(integrity(
                        "published stream ended before its declared length",
                    ));
                }
                finish_controls(
                    stage.write_all(&buffer[..n]).map_err(Into::into),
                    control,
                    supplemental,
                )?;
                remaining -= n as u64;
            }
            check_with(control, supplemental)?;
            if finish_controls(
                reader.read(&mut buffer[..1]).map_err(io_error),
                control,
                supplemental,
            )? != 0
            {
                return Err(limit("output source exceeds its declared length"));
            }
            Ok(())
        },
    )
}
pub(super) fn publish_bytes(
    home: &Path,
    path: &Path,
    bytes: &[u8],
    reference: &BlobRef,
    maximum: u64,
    control: Option<&OperationControl>,
    supplemental: &mut impl FnMut() -> Result<()>,
    settlement: &mut BlobPublicationSettlement,
) -> Result<()> {
    publish_stage(
        home,
        path,
        reference,
        maximum,
        control,
        supplemental,
        settlement,
        |stage, supplemental| {
            for block in bytes.chunks(BLOCK) {
                check_with(control, supplemental)?;
                finish_controls(
                    stage.write_all(block).map_err(Into::into),
                    control,
                    supplemental,
                )?;
            }
            Ok(())
        },
    )
}
/// Same-directory original-handle publication. Every acquired stage and parent
/// is explicitly consumed, including cancellation immediately after creation.
#[allow(clippy::too_many_arguments)]
fn publish_stage<C: FnMut() -> Result<()>>(
    home: &Path,
    path: &Path,
    reference: &BlobRef,
    maximum: u64,
    control: Option<&OperationControl>,
    supplemental: &mut C,
    settlement: &mut BlobPublicationSettlement,
    copy: impl FnOnce(&mut StagedFile<'_>, &mut C) -> Result<()>,
) -> Result<()> {
    check_with(control, supplemental)?;
    if reference.bytes > maximum {
        return Err(limit("output exceeds publication budget"));
    }
    let parent = Directory::open_controlled(
        path.parent().expect("blob shard"),
        &mut PathControl {
            control,
            supplemental,
        },
    )?;
    let mut stage = None;
    let mut publication = staged::Publication::new();
    publication.reference = Some(reference.clone());
    let result = (|| {
        stage = Some(StagedFile::create(
            &parent,
            &mut PathControl {
                control,
                supplemental,
            },
        )?);
        settlement.stage_acquired();
        publication.stage_name = Some(stage.as_ref().expect("owned stage").name().into());
        check_with(control, supplemental)?;
        copy(stage.as_mut().expect("owned stage"), supplemental)?;
        check_with(control, supplemental)?;
        finish_controls(
            stage
                .as_mut()
                .expect("owned stage")
                .seal()
                .map_err(Into::into),
            control,
            supplemental,
        )?;
        let published = stage.as_mut().expect("owned stage").publish_name(
            path.file_name().expect("blob name"),
            &mut PathControl {
                control,
                supplemental,
            },
        );
        publication.state = stage.as_ref().expect("owned stage").publication();
        if finish_controls(published, control, supplemental)? == StagedPublication::NotPublished {
            let opened = open_blob_with_check(home, reference.clone(), control, supplemental)?;
            scan_blob_with_check(opened, control, supplemental, |_| Ok(()))?;
            publication.reused = true;
        }
        Ok(())
    })();
    settlement.publication = publication.state;
    settlement.reused = publication.reused;
    let mut result = result;
    if let Some(owned) = stage.take() {
        let cleanup = owned.close();
        settlement.stage_closed(cleanup.is_empty());
        for error in cleanup {
            result =
                crate::metadata::historical_write::finish_historical(result, Err(error.into()));
        }
    }
    drop(stage);
    let result = optional::finish_directory(result, parent);
    finish_controls(result, control, supplemental).map_err(|error| publication.annotate(error))
}

/// Keep an already-known immutable manifest when a later stop/close fails.
/// Only the SQLite writer may mark Committed; Unknown never authorizes retry.
pub(in crate::evidence) fn capture_result(
    result: Result<()>,
    value: Value,
    state: crate::metadata::CaptureWriteState,
) -> Result<Value> {
    use crate::metadata::CaptureWriteState;
    match result {
        Ok(()) => Ok(value),
        Err(error) if state == CaptureWriteState::NotCommitted => Err(error),
        Err(mut error) => {
            let original = error.clone();
            let id = value["id"].as_str().unwrap_or("").to_owned();
            let committed = state == CaptureWriteState::Committed;
            error.message = format!(
                "{}; capture {} {}",
                error.message,
                id,
                if committed {
                    "is committed"
                } else {
                    "commit state is unknown; do not replay"
                }
            );
            error.details = serde_json::json!({
                "schema": "lycheedev.capture-commit-error.v1",
                "primary": original,
                "capturePublication": {"state": if committed {"committed"} else {"unknown"}, "id": id},
                "completedCaptures": if committed {vec![id]} else {Vec::<String>::new()},
            });
            Err(error)
        }
    }
}
pub(in crate::evidence) fn close_directory<T>(
    result: Result<T>,
    directory: Directory,
) -> Result<T> {
    optional::finish_directory(result, directory)
}

pub(super) fn persist_capture(
    home: &Path,
    key: &str,
    raw: &[u8],
    control: Option<&OperationControl>,
    supplemental: &mut impl FnMut() -> Result<()>,
    state: &std::cell::Cell<crate::metadata::CaptureWriteState>,
) -> Result<()> {
    let mut metadata = match control {
        Some(control) => {
            Metadata::open_controlled_with_check(home, true, control, &mut *supplemental)?
        }
        None => Metadata::open(home, true)?,
    };
    let written = match control {
        Some(control) => metadata.put_capture_serialized(
            key,
            raw,
            &[],
            control,
            &mut *supplemental,
            |_| Ok(()),
            |_, _| Ok(()),
            || Ok(()),
            state,
        ),
        None => {
            let result = metadata.put_serialized(key, raw, None, &mut *supplemental);
            if result.is_ok() {
                state.set(crate::metadata::CaptureWriteState::Committed);
            }
            result
        }
    };
    let result = crate::metadata::historical_write::finish_historical(written, metadata.close());
    finish_controls(result, control, supplemental)
}

#[cfg(test)]
mod tests;
