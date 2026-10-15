//! One source traversal, one private stage, and no digest-name publication until
//! the producer has returned a real nonempty-read EOF (including business checks).
use super::super::CaptureAccounting;
use super::*;
use lycheedev_windows::filesystem::destination::{
    FileControl, FileCost, StagedFile, StagedPublication,
};

struct Checks<'a> {
    control: &'a OperationControl,
    supplemental: &'a mut Option<BlobCheck>,
}
impl FileControl for Checks<'_> {
    type Error = ToolkitError;
    fn charge(&mut self, _: FileCost) -> Result<()> {
        self.check()
    }
    fn check(&mut self) -> Result<()> {
        stream_check(self.control, self.supplemental)
    }
}
struct Publication {
    state: StagedPublication,
    reference: Option<BlobRef>,
    stage_name: Option<String>,
    source_stage_name: Option<String>,
    reused: bool,
}
impl Publication {
    fn new() -> Self {
        Self {
            state: StagedPublication::NotPublished,
            reference: None,
            stage_name: None,
            source_stage_name: None,
            reused: false,
        }
    }
    fn annotate(&self, mut error: ToolkitError) -> ToolkitError {
        // Preserve a source's structured missingKey and every compound cleanup
        // field. Annotation must not flatten or replace the original cause.
        if !error.details.is_object() {
            let original = std::mem::replace(&mut error.details, serde_json::json!({}));
            if !original.is_null() {
                error.details["sourceDetails"] = original;
            }
        }
        let attempt = match self.state {
            StagedPublication::NotPublished => "not_published",
            StagedPublication::Published => "published",
            StagedPublication::Unknown => "unknown",
        };
        // A producer or metering error can already carry a publication fact.
        // Never turn its Published/Unknown into permission to replay. Keep the
        // complete original object while separately recording this attempt.
        let prior = error.details.get("publication").cloned();
        let prior_state = prior.as_ref().and_then(|p| p["state"].as_str());
        let state = if attempt == "unknown"
            || prior.is_some() && !matches!(prior_state, Some("not_published" | "published"))
        {
            "unknown"
        } else if attempt == "published" || prior_state == Some("published") {
            "published"
        } else {
            "not_published"
        };
        error.details["publication"] = serde_json::json!({
            "state": state,
            "attemptState": attempt,
            "prior": prior,
            "reference": self.reference,
            "stageName": self.stage_name,
            "sourceStageName": self.source_stage_name,
            "reused": self.reused,
        });
        error
    }
}

/// Stage a known-length stream whose digest is not yet known. The source must
/// finish all format/content validation before its nonempty Read returns zero.
/// Core always probes one extra byte after the exact length, even when another
/// publisher has already stored the final digest. The source is never replayed.
///
/// After source EOF, a sealed original-handle spool is copied into a second
/// private stage within the final shard; only same-directory rename is used.
/// The three binary passes run sequentially with one 64 KiB Core body buffer
/// live at a time. The caller admits that buffer with its retained reservation;
/// this borrowed meter admits per-read work only and never escapes into the blob.
/// Extra disk files/copies do not count as Go logical decode/blob/network reads.
/// At most 64 KiB is read at once. Actual-byte/network/decode accounting belongs
/// to the producer; this function neither replaces that ledger nor decodes.
/// The owned supplemental check remains alive through returned Read/rewind/close.
/// Errors preserve their typed cause and add publication state. Only explicit
/// not_published permits a caller to consider its own safe recovery policy;
/// published/unknown must never cause an automatic source or commit replay.
pub fn publish_blob_stream_bounded_with_check(
    home: &Path,
    reader: &mut impl Read,
    expected_bytes: u64,
    maximum: u64,
    control: &OperationControl,
    supplemental: impl FnMut() -> Result<()> + Send + 'static,
    accounting: &impl CaptureAccounting,
) -> Result<VerifiedBlob> {
    let mut supplemental: Option<BlobCheck> = Some(Box::new(supplemental));
    let mut publication = Publication::new();
    let result = publish_stream(
        home,
        reader,
        expected_bytes,
        maximum,
        control,
        &mut supplemental,
        &mut publication,
        accounting,
    );
    match result {
        Ok(opened) => Ok(VerifiedBlob {
            opened,
            home: home.to_owned(),
            control: control.clone(),
            supplemental,
        }),
        Err(error) => Err(publication.annotate(error)),
    }
}

#[allow(clippy::too_many_arguments)]
fn publish_stream(
    home: &Path,
    reader: &mut impl Read,
    expected_bytes: u64,
    maximum: u64,
    control: &OperationControl,
    supplemental: &mut Option<BlobCheck>,
    publication: &mut Publication,
    accounting: &impl CaptureAccounting,
) -> Result<OpenBlob> {
    stream_check(control, supplemental)?;
    if expected_bytes > maximum || expected_bytes > MAX_CAPTURE_BYTES {
        return Err(limit("stream exceeds publication budget"));
    }
    workspace::open(home)?;
    let parent = Directory::open_controlled(
        &home.join("blobs"),
        &mut Checks {
            control,
            supplemental,
        },
    )?;
    let mut staged = None;
    let mut guard = None;
    let result = (|| {
        staged = Some(StagedFile::create(
            &parent,
            &mut Checks {
                control,
                supplemental,
            },
        )?);
        publication.stage_name = Some(staged.as_ref().expect("owned stage").name().to_owned());
        publication.source_stage_name = publication.stage_name.clone();
        // Ownership exists before this post-acquisition stop, so every failure
        // takes the original-handle discard/checked-close path below.
        stream_check(control, supplemental)?;
        let reference = copy_stream(
            reader,
            staged.as_mut().expect("owned stage"),
            expected_bytes,
            control,
            supplemental,
            accounting,
        )?;
        publication.reference = Some(reference.clone());
        stream_check(control, supplemental)?;
        let synced = staged
            .as_mut()
            .expect("owned stage")
            .seal()
            .map_err(ToolkitError::from);
        finish_checks(synced, control, supplemental)?;
        guard = Some(ExclusiveLock::resource_wait_checked(
            &home.join("locks"),
            &format!("blob:{}", reference.sha256),
            Some(control.deadline()),
            || stream_check(control, supplemental),
        )?);
        publish_in_shard(
            home,
            &parent,
            &mut staged,
            reference,
            control,
            supplemental,
            publication,
            accounting,
        )
    })();
    let result = finish_opened(result, close_stage(&mut staged), control, supplemental);
    let result = if let Some(guard) = guard {
        finish_opened(result, guard.close(), control, supplemental)
    } else {
        result
    };
    // No stage borrow remains when the original staging parent is consumed.
    drop(staged);
    let result = finish_opened(
        result,
        optional::finish_directory(Ok(()), parent),
        control,
        supplemental,
    );
    let checked = finish_checks(Ok(()), control, supplemental);
    finish_opened(result, checked, control, supplemental)
}
fn finish_checks<T>(
    result: Result<T>,
    control: &OperationControl,
    supplemental: &mut Option<BlobCheck>,
) -> Result<T> {
    optional::finish_checks(result, control, &mut || optional_check(supplemental))
}
fn close_stage(stage: &mut Option<StagedFile<'_>>) -> Result<()> {
    let Some(stage) = stage.take() else {
        return Ok(());
    };
    let errors = stage.close();
    let mut result = Ok(());
    for error in errors {
        result = crate::metadata::historical_write::finish_historical(
            result,
            Err(ToolkitError::from(error)),
        );
    }
    #[cfg(test)]
    {
        result = crate::metadata::historical_write::finish_historical(
            result,
            test_hooks::after_stage_close(),
        );
    }
    result
}
fn finish_opened(
    result: Result<OpenBlob>,
    cleanup: Result<()>,
    control: &OperationControl,
    supplemental: &mut Option<BlobCheck>,
) -> Result<OpenBlob> {
    match (result, cleanup) {
        (Ok(opened), Ok(())) => Ok(opened),
        (Ok(opened), Err(error)) => close_opened_blob(opened, Err(error), control, supplemental),
        (Err(error), cleanup) => {
            crate::metadata::historical_write::finish_historical(Err(error), cleanup)
        }
    }
}
fn copy_stream(
    reader: &mut impl Read,
    stage: &mut StagedFile<'_>,
    expected_bytes: u64,
    control: &OperationControl,
    supplemental: &mut Option<BlobCheck>,
    accounting: &impl CaptureAccounting,
) -> Result<BlobRef> {
    let mut buffer = [0u8; BLOCK];
    let mut remaining = expected_bytes;
    let mut digest = Sha256::new();
    while remaining != 0 {
        stream_check(control, supplemental)?;
        let admitted = remaining.min(BLOCK as u64) as usize;
        accounting.admit_work(1)?;
        let read = reader
            .read(&mut buffer[..admitted])
            .map_err(io_error)
            .and_then(|n| {
                if n == 0 {
                    Err(integrity("stream ended before its declared length"))
                } else {
                    Ok(n)
                }
            });
        let n = finish_checks(read, control, supplemental)?;
        digest.update(&buffer[..n]);
        let written = stage.write_all(&buffer[..n]).map_err(ToolkitError::from);
        finish_checks(written, control, supplemental)?;
        remaining -= n as u64;
    }
    stream_check(control, supplemental)?;
    // A nonempty EOF probe is also the producer's business-validation callback.
    // Never optimize this away using length or an already-published digest.
    accounting.admit_work(1)?;
    let eof = reader
        .read(&mut buffer[..1])
        .map_err(io_error)
        .and_then(|n| {
            if n == 0 {
                Ok(())
            } else {
                Err(integrity("stream exceeds its declared length"))
            }
        });
    finish_checks(eof, control, supplemental)?;
    Ok(BlobRef {
        sha256: hex::encode(digest.finalize()),
        bytes: expected_bytes,
    })
}
#[allow(clippy::too_many_arguments)]
fn publish_in_shard(
    home: &Path,
    parent: &Directory,
    spool: &mut Option<StagedFile<'_>>,
    reference: BlobRef,
    control: &OperationControl,
    supplemental: &mut Option<BlobCheck>,
    publication: &mut Publication,
    accounting: &impl CaptureAccounting,
) -> Result<OpenBlob> {
    let path = blob_path(home, &reference.sha256)?;
    storage::guard_path(home, &path)?;
    parent.verify_controlled(&mut Checks {
        control,
        supplemental,
    })?;
    let shard = path.parent().expect("blob shard");
    stream_check(control, supplemental)?;
    let created = match fs::create_dir(shard) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error.into()),
    };
    finish_checks(created, control, supplemental)?;
    storage::guard_path(home, &path)?;
    let directory = Directory::open_controlled(
        shard,
        &mut Checks {
            control,
            supplemental,
        },
    )?;
    let mut destination = None;
    let result = (|| {
        let name = path.file_name().expect("digest name");
        if let Some(file) = open_winner(&directory, name, &reference, control, supplemental)? {
            publication.reused = true;
            return Ok(file);
        }
        destination = Some(StagedFile::create(
            &directory,
            &mut Checks {
                control,
                supplemental,
            },
        )?);
        let stage = destination.as_mut().expect("owned final stage");
        publication.stage_name = Some(format!("{}/{}", &reference.sha256[..2], stage.name()));
        stream_check(control, supplemental)?;
        // A second bounded *disk* pass, reading the original sealed handle.
        // The Data reader and its business EOF callback are never invoked here.
        // This copy owns the only live Core body buffer at this phase.
        let copied = copy_stream(
            spool.as_mut().expect("owned sealed spool"),
            stage,
            reference.bytes,
            control,
            supplemental,
            accounting,
        )?;
        if copied.sha256 != reference.sha256 {
            return Err(integrity("sealed spool digest differs"));
        }
        stream_check(control, supplemental)?;
        let sealed = stage.seal().map_err(ToolkitError::from);
        finish_checks(sealed, control, supplemental)?;
        #[cfg(test)]
        test_hooks::before_rename();
        let committed = stage.publish_name(
            name,
            &mut Checks {
                control,
                supplemental,
            },
        );
        publication.state = stage.publication();
        let committed = finish_checks(committed, control, supplemental)?;
        publication.reused = committed == StagedPublication::NotPublished;
        // Preserve its Published/Unknown fact before consuming this handle.
        // A fresh immutable reader cannot open while this write handle is live.
        close_stage(&mut destination)?;
        stream_check(control, supplemental)?;
        open_winner(&directory, name, &reference, control, supplemental)?
            .ok_or_else(|| integrity("published blob or concurrent winner disappeared"))
    })();
    // Both stages have separate original-handle cleanup. The root spool remains
    // owned by publish_stream even if this final stage is Unknown.
    let cleanup = close_stage(&mut destination);
    let result = match (result, cleanup) {
        (Ok(file), Ok(())) => Ok(file),
        (Ok(file), Err(error)) => crate::metadata::historical_write::finish_historical(
            Err(error),
            lycheedev_windows::filesystem::close_file(file).map_err(ToolkitError::from),
        ),
        (Err(error), cleanup) => {
            crate::metadata::historical_write::finish_historical(Err(error), cleanup)
        }
    };
    drop(destination);
    match result {
        Ok(file) => verify_opened(
            OpenBlob {
                file,
                _directory: directory,
                reference,
            },
            control,
            supplemental,
            accounting,
        ),
        Err(error) => optional::finish_directory(Err(error), directory),
    }
}
fn verify_opened(
    mut opened: OpenBlob,
    control: &OperationControl,
    supplemental: &mut Option<BlobCheck>,
    accounting: &impl CaptureAccounting,
) -> Result<OpenBlob> {
    let result = (|| {
        scan_open_blob_accounted_with_check(
            &mut opened,
            Some(control),
            &mut || optional_check(supplemental),
            |_| Ok(()),
            || accounting.admit_work(1),
        )?;
        stream_check(control, supplemental)?;
        let rewound = opened
            .file
            .seek(SeekFrom::Start(0))
            .map(|_| ())
            .map_err(ToolkitError::from);
        finish_checks(rewound, control, supplemental)
    })();
    match result {
        Ok(()) => Ok(opened),
        Err(error) => close_opened_blob(opened, Err(error), control, supplemental),
    }
}
fn open_winner(
    directory: &Directory,
    name: &std::ffi::OsStr,
    reference: &BlobRef,
    control: &OperationControl,
    supplemental: &mut Option<BlobCheck>,
) -> Result<Option<File>> {
    let info = directory.optional_metadata_controlled(
        name,
        &mut Checks {
            control,
            supplemental,
        },
    )?;
    let Some(info) = info else {
        return Ok(None);
    };
    validate_metadata(&info, reference.bytes)?;
    let Some(file) = directory.open_optional_file_controlled(
        name,
        &mut Checks {
            control,
            supplemental,
        },
    )?
    else {
        return Ok(None);
    };
    let result = file
        .metadata()
        .map_err(ToolkitError::from)
        .and_then(|info| validate_metadata(&info, reference.bytes));
    match finish_checks(result, control, supplemental) {
        Ok(()) => Ok(Some(file)),
        Err(error) => crate::metadata::historical_write::finish_historical(
            Err(error),
            lycheedev_windows::filesystem::close_file(file).map_err(ToolkitError::from),
        ),
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod test_hooks {
    use super::*;
    use std::cell::RefCell;
    thread_local! {
        static BEFORE_RENAME: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
        static AFTER_STAGE_CLOSE: RefCell<Option<Box<dyn FnOnce() -> Result<()>>>> = RefCell::new(None);
    }
    pub(super) fn set_before_rename(run: impl FnOnce() + 'static) {
        BEFORE_RENAME.with(|s| {
            assert!(s.borrow().is_none());
            *s.borrow_mut() = Some(Box::new(run));
        });
    }
    pub(super) fn before_rename() {
        if let Some(run) = BEFORE_RENAME.with(|s| s.borrow_mut().take()) {
            run();
        }
    }
    pub(super) fn set_after_stage_close(run: impl FnOnce() -> Result<()> + 'static) {
        AFTER_STAGE_CLOSE.with(|s| {
            assert!(s.borrow().is_none());
            *s.borrow_mut() = Some(Box::new(run));
        });
    }
    pub(super) fn after_stage_close() -> Result<()> {
        AFTER_STAGE_CLOSE
            .with(|s| s.borrow_mut().take())
            .map_or(Ok(()), |run| run())
    }
}
