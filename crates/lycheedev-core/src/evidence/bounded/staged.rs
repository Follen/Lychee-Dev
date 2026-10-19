//! One source traversal, one private stage, and no digest-name publication until
//! the producer has returned a real nonempty-read EOF (including business checks).
use super::super::{BlobStreamPhase, CaptureAccounting};
use super::*;
use lycheedev_windows::filesystem::destination::{
    FileControl, FileCost, PrivateSpool, StagedFile, StagedPublication,
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
/// Typed final-shard and private-spool cleanup, never inferred from JSON errors.
pub(crate) struct StreamPublicationSettlement {
    pub(crate) publication: StagedPublication,
    pub(crate) reused: bool,
    pub(crate) spool_cleanup_complete: bool,
    pub(crate) shard_cleanup_complete: bool,
}
impl StreamPublicationSettlement {
    pub(crate) fn new() -> Self {
        Self {
            publication: StagedPublication::NotPublished,
            reused: false,
            spool_cleanup_complete: false,
            shard_cleanup_complete: true,
        }
    }
    pub(crate) fn may_release_unpublished_intent(&self) -> bool {
        self.publication == StagedPublication::NotPublished
            && !self.reused
            && self.spool_cleanup_complete
            && self.shard_cleanup_complete
    }
    pub(crate) fn summary(&self) -> serde_json::Value {
        serde_json::json!({"publication":format!("{:?}", self.publication), "reused":self.reused,
            "spoolCleanupComplete":self.spool_cleanup_complete,
            "shardCleanupComplete":self.shard_cleanup_complete})
    }
}
pub(super) struct Publication {
    pub(super) state: StagedPublication,
    pub(super) reference: Option<BlobRef>,
    pub(super) stage_name: Option<String>,
    pub(super) source_stage_name: Option<String>,
    source_stage_path: Option<PathBuf>,
    pub(super) reused: bool,
}
impl Publication {
    pub(super) fn new() -> Self {
        Self {
            state: StagedPublication::NotPublished,
            reference: None,
            stage_name: None,
            source_stage_name: None,
            source_stage_path: None,
            reused: false,
        }
    }
    pub(super) fn annotate(&self, mut error: ToolkitError) -> ToolkitError {
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
        if let Some(path) = &self.source_stage_path {
            error.details["publication"]["sourceStagePath"] = serde_json::json!(path);
        }
        error
    }
}

/// Stage a known-length stream whose digest is not yet known. The source must
/// finish all format/content validation before its nonempty Read returns zero.
/// Core always probes one extra byte after the exact length, even when another
/// publisher has already stored the final digest. The source is never replayed.
///
/// After source EOF, a nonpublishable original-handle spool becomes read-ready
/// without requesting crash durability. It has no recovery consumer. Its bytes
/// are copied and rehashed into a different, durably sealed final-shard stage;
/// only same-directory rename is used. Private-spool-only sync errors are no
/// longer requested; real read/write/seek/final-sync and checked-close failures
/// still terminate this call. Final sync guarantees the final object, not the
/// crash survival of the disposable spool.
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
        None,
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
pub(crate) fn publish_data_blob_stream_with_check(
    home: &Path,
    reader: &mut impl Read,
    expected_bytes: u64,
    maximum: u64,
    control: &OperationControl,
    supplemental: impl FnMut() -> Result<()> + Send + 'static,
    accounting: &impl CaptureAccounting,
    quota: &crate::workspace::data_disk::StreamQuota<'_>,
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
        Some(quota),
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
    quota: Option<&crate::workspace::data_disk::StreamQuota<'_>>,
) -> Result<OpenBlob> {
    stream_check(control, supplemental)?;
    if expected_bytes > maximum || expected_bytes > MAX_CAPTURE_BYTES {
        return Err(limit("stream exceeds publication budget"));
    }
    open_workspace(home, Some(control), &mut || optional_check(supplemental))?;
    // New Data spools live inside the already measured staging root. Their
    // exact logical length is reserved once; producer pulls hold no quota lock.
    let mut initial = quota.map(|q| q.reserve_spool(expected_bytes)).transpose()?;
    let spool_path = match &initial {
        Some(owner) => owner.paths().staging.clone(),
        None => home.join("blobs"),
    };
    let parent = Directory::open_controlled(
        &spool_path,
        &mut Checks {
            control,
            supplemental,
        },
    );
    let parent = match parent {
        Ok(parent) => parent,
        Err(error) => {
            return match initial.take() {
                Some(owner) => owner.finish(Err(error)),
                None => Err(error),
            };
        }
    };
    let mut staged = None;
    let mut guard = None;
    let result = (|| {
        staged = Some({
            let _phase = accounting.blob_stream_phase(BlobStreamPhase::StageCreate);
            PrivateSpool::create(
                &parent,
                &mut Checks {
                    control,
                    supplemental,
                },
            )?
        });
        publication.stage_name = Some(staged.as_ref().expect("owned stage").name().to_owned());
        publication.source_stage_name = publication.stage_name.clone();
        if quota.is_some() {
            publication.source_stage_path =
                Some(spool_path.join(staged.as_ref().expect("owned stage").name()));
        }
        if let Some(owner) = initial.take() {
            let reserved = (|| {
                stream_check(control, supplemental)?;
                staged
                    .as_mut()
                    .expect("owned stage")
                    .reserve_exact_len(expected_bytes)?;
                stream_check(control, supplemental)
            })();
            owner.finish(reserved)?;
        }
        // Ownership exists before this post-acquisition stop, so every failure
        // takes the original-handle discard/checked-close path below.
        stream_check(control, supplemental)?;
        let source_phase = accounting.blob_stream_phase(BlobStreamPhase::SourceSpool);
        let reference = copy_stream(
            reader,
            &mut |bytes| staged.as_mut().expect("owned stage").write_all(bytes),
            expected_bytes,
            control,
            supplemental,
            accounting,
        )?;
        drop(source_phase);
        publication.reference = Some(reference.clone());
        stream_check(control, supplemental)?;
        let readable = {
            let _phase = accounting.blob_stream_phase(BlobStreamPhase::PrivateSpoolReadReady);
            let result = staged
                .as_mut()
                .expect("owned stage")
                .finish_writes()
                .map_err(ToolkitError::from);
            #[cfg(test)]
            let result = result.and_then(|()| test_hooks::after_private_read_ready());
            result
        };
        finish_checks(readable, control, supplemental)?;
        if let Some(quota) = quota {
            let mut opened = None;
            let result = quota.publish(&reference, |settlement, reuse_only| {
                let mut blob_parent = None;
                let result = (|| {
                    blob_parent = Some(Directory::open_controlled(
                        &home.join("blobs"),
                        &mut Checks {
                            control,
                            supplemental,
                        },
                    )?);
                    guard = Some({
                        let _phase = accounting.blob_stream_phase(BlobStreamPhase::DigestLock);
                        ExclusiveLock::resource_wait_checked(
                            &home.join("locks"),
                            &format!("blob:{}", reference.sha256),
                            Some(control.deadline()),
                            || stream_check(control, supplemental),
                        )?
                    });
                    publish_in_shard(
                        home,
                        blob_parent.as_ref().expect("held blob parent"),
                        &mut staged,
                        reference.clone(),
                        control,
                        supplemental,
                        publication,
                        accounting,
                        Some(&mut *settlement),
                        reuse_only,
                    )
                })();
                settlement.publication = publication.state;
                let cleanup = close_spool(&mut staged, accounting);
                settlement.spool_cleanup_complete = cleanup.is_ok();
                let result = finish_opened(result, cleanup, control, supplemental);
                let result = match guard.take() {
                    Some(guard) => {
                        let cleanup = {
                            let _phase =
                                accounting.blob_stream_phase(BlobStreamPhase::CheckedClose);
                            guard.close()
                        };
                        finish_opened(result, cleanup, control, supplemental)
                    }
                    None => result,
                };
                let result = match blob_parent {
                    Some(parent) => finish_opened(
                        result,
                        optional::finish_directory(Ok(()), parent),
                        control,
                        supplemental,
                    ),
                    None => result,
                };
                match result {
                    Ok(blob) => {
                        opened = Some(blob);
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            });
            return match opened {
                Some(blob) => finish_opened(Ok(blob), result, control, supplemental),
                None => match result {
                    Err(error) => Err(error),
                    Ok(()) => Err(integrity("Data stream settled without an opened body")),
                },
            };
        }
        guard = Some({
            let _phase = accounting.blob_stream_phase(BlobStreamPhase::DigestLock);
            ExclusiveLock::resource_wait_checked(
                &home.join("locks"),
                &format!("blob:{}", reference.sha256),
                Some(control.deadline()),
                || stream_check(control, supplemental),
            )?
        });
        publish_in_shard(
            home,
            &parent,
            &mut staged,
            reference,
            control,
            supplemental,
            publication,
            accounting,
            None,
            false,
        )
    })();
    let result = finish_opened(
        result,
        close_spool(&mut staged, accounting),
        control,
        supplemental,
    );
    let result = if let Some(owner) = initial.take() {
        // No body can be opened before the initial reservation is released.
        match result {
            Err(error) => owner.finish(Err(error)),
            Ok(opened) => finish_opened(Ok(opened), owner.finish(Ok(())), control, supplemental),
        }
    } else {
        result
    };
    let result = if let Some(guard) = guard {
        let cleanup = {
            let _phase = accounting.blob_stream_phase(BlobStreamPhase::CheckedClose);
            guard.close()
        };
        finish_opened(result, cleanup, control, supplemental)
    } else {
        result
    };
    // No stage borrow remains when the original staging parent is consumed.
    drop(staged);
    let cleanup = {
        let _phase = accounting.blob_stream_phase(BlobStreamPhase::CheckedClose);
        optional::finish_directory(Ok(()), parent)
    };
    let result = finish_opened(result, cleanup, control, supplemental);
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
fn close_stage(
    stage: &mut Option<StagedFile<'_>>,
    accounting: &impl CaptureAccounting,
) -> Result<()> {
    let Some(stage) = stage.take() else {
        return Ok(());
    };
    let _phase = accounting.blob_stream_phase(BlobStreamPhase::CheckedClose);
    close_errors(stage.close())
}
fn close_spool(
    stage: &mut Option<PrivateSpool<'_>>,
    accounting: &impl CaptureAccounting,
) -> Result<()> {
    let Some(stage) = stage.take() else {
        return Ok(());
    };
    let _phase = accounting.blob_stream_phase(BlobStreamPhase::CheckedClose);
    close_errors(stage.close())
}
fn close_errors(errors: Vec<io::Error>) -> Result<()> {
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
pub(super) fn finish_opened(
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
    write: &mut impl FnMut(&[u8]) -> io::Result<()>,
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
        #[cfg(test)]
        let read = test_hooks::before_stream_read()
            .and_then(|()| reader.read(&mut buffer[..admitted]).map_err(io_error));
        #[cfg(not(test))]
        let read = reader.read(&mut buffer[..admitted]).map_err(io_error);
        let read = read.and_then(|n| {
            if n == 0 {
                Err(integrity("stream ended before its declared length"))
            } else {
                Ok(n)
            }
        });
        let n = finish_checks(read, control, supplemental)?;
        digest.update(&buffer[..n]);
        #[cfg(test)]
        let written = test_hooks::before_stage_write()
            .unwrap_or_else(|| write(&buffer[..n]).map_err(ToolkitError::from));
        #[cfg(not(test))]
        let written = write(&buffer[..n]).map_err(ToolkitError::from);
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
    spool: &mut Option<PrivateSpool<'_>>,
    reference: BlobRef,
    control: &OperationControl,
    supplemental: &mut Option<BlobCheck>,
    publication: &mut Publication,
    accounting: &impl CaptureAccounting,
    mut settlement: Option<&mut StreamPublicationSettlement>,
    reuse_only: bool,
) -> Result<OpenBlob> {
    let shard_phase = accounting.blob_stream_phase(BlobStreamPhase::ShardOpen);
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
    drop(shard_phase);
    let mut destination = None;
    let result = (|| {
        let name = path.file_name().expect("digest name");
        if let Some(file) = open_winner(
            &directory,
            name,
            &reference,
            control,
            supplemental,
            accounting,
        )? {
            publication.reused = true;
            return Ok(file);
        }
        if reuse_only {
            return Err(integrity(
                "registered Data body disappeared; read-only reuse cannot republish",
            ));
        }
        destination = Some({
            let _phase = accounting.blob_stream_phase(BlobStreamPhase::StageCreate);
            StagedFile::create(
                &directory,
                &mut Checks {
                    control,
                    supplemental,
                },
            )?
        });
        if let Some(settlement) = settlement.as_deref_mut() {
            settlement.shard_cleanup_complete = false;
        }
        let stage = destination.as_mut().expect("owned final stage");
        publication.stage_name = Some(format!("{}/{}", &reference.sha256[..2], stage.name()));
        stream_check(control, supplemental)?;
        // A second bounded *disk* pass, reading the original sealed handle.
        // The Data reader and its business EOF callback are never invoked here.
        // This copy owns the only live Core body buffer at this phase.
        let copy_phase = accounting.blob_stream_phase(BlobStreamPhase::ShardCopy);
        let copied = copy_stream(
            spool.as_mut().expect("owned sealed spool"),
            &mut |bytes| stage.write_all(bytes),
            reference.bytes,
            control,
            supplemental,
            accounting,
        )?;
        if copied.sha256 != reference.sha256 {
            return Err(integrity("sealed spool digest differs"));
        }
        drop(copy_phase);
        stream_check(control, supplemental)?;
        let sealed = {
            let _phase = accounting.blob_stream_phase(BlobStreamPhase::FinalStageSeal);
            let result = stage.seal().map_err(ToolkitError::from);
            #[cfg(test)]
            let result = result.and_then(|()| test_hooks::after_final_stage_seal());
            result
        };
        finish_checks(sealed, control, supplemental)?;
        #[cfg(test)]
        test_hooks::before_rename();
        let committed = {
            let _phase = accounting.blob_stream_phase(BlobStreamPhase::Rename);
            stage.publish_name(
                name,
                &mut Checks {
                    control,
                    supplemental,
                },
            )
        };
        publication.state = stage.publication();
        let committed = finish_checks(committed, control, supplemental)?;
        publication.reused = committed == StagedPublication::NotPublished;
        // Preserve its Published/Unknown fact before consuming this handle.
        // A fresh immutable reader cannot open while this write handle is live.
        let cleanup = close_stage(&mut destination, accounting);
        if let Some(settlement) = settlement.as_deref_mut() {
            settlement.shard_cleanup_complete = cleanup.is_ok();
        }
        cleanup?;
        stream_check(control, supplemental)?;
        open_winner(
            &directory,
            name,
            &reference,
            control,
            supplemental,
            accounting,
        )?
        .ok_or_else(|| integrity("published blob or concurrent winner disappeared"))
    })();
    // Both stages have separate original-handle cleanup. The root spool remains
    // owned by publish_stream even if this final stage is Unknown.
    let had_destination = destination.is_some();
    let cleanup = close_stage(&mut destination, accounting);
    if had_destination {
        if let Some(settlement) = settlement.as_deref_mut() {
            settlement.shard_cleanup_complete = cleanup.is_ok();
        }
    }
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
        Ok(file) => {
            let verified = verify_opened(
                OpenBlob {
                    file,
                    _directory: directory,
                    reference,
                },
                control,
                supplemental,
                accounting,
            );
            if verified.is_ok() && publication.reused {
                if let Some(settlement) = settlement.as_deref_mut() {
                    // Public diagnostics describe the rename/winner branch;
                    // quota release needs a separately authenticated fact.
                    settlement.reused = true;
                }
            }
            verified
        }
        Err(error) => {
            let _phase = accounting.blob_stream_phase(BlobStreamPhase::CheckedClose);
            optional::finish_directory(Err(error), directory)
        }
    }
}
fn verify_opened(
    mut opened: OpenBlob,
    control: &OperationControl,
    supplemental: &mut Option<BlobCheck>,
    accounting: &impl CaptureAccounting,
) -> Result<OpenBlob> {
    let _phase = accounting.blob_stream_phase(BlobStreamPhase::FinalVerify);
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
    accounting: &impl CaptureAccounting,
) -> Result<Option<File>> {
    let _phase = accounting.blob_stream_phase(BlobStreamPhase::WinnerOpen);
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
mod failure_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod test_hooks {
    use super::*;
    use std::cell::RefCell;
    thread_local! {
        static BEFORE_RENAME: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
        static AFTER_STAGE_CLOSE: RefCell<Option<Box<dyn FnOnce() -> Result<()>>>> = RefCell::new(None);
        static BEFORE_STAGE_WRITE: RefCell<Option<Box<dyn FnOnce() -> Result<()>>>> = RefCell::new(None);
        static AFTER_PRIVATE_READ_READY: RefCell<Option<Box<dyn FnOnce() -> Result<()>>>> = RefCell::new(None);
        static AFTER_FINAL_STAGE_SEAL: RefCell<Option<Box<dyn FnOnce() -> Result<()>>>> = RefCell::new(None);
        static BEFORE_STREAM_READ: RefCell<Option<Box<dyn FnOnce() -> Result<()>>>> = RefCell::new(None);
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
    pub(super) fn set_before_stage_write(run: impl FnOnce() -> Result<()> + 'static) {
        BEFORE_STAGE_WRITE.with(|s| {
            assert!(s.borrow().is_none());
            *s.borrow_mut() = Some(Box::new(run));
        });
    }
    pub(super) fn before_stage_write() -> Option<Result<()>> {
        BEFORE_STAGE_WRITE
            .with(|s| s.borrow_mut().take())
            .map(|run| run())
    }
    pub(super) fn set_after_private_read_ready(run: impl FnOnce() -> Result<()> + 'static) {
        AFTER_PRIVATE_READ_READY.with(|s| {
            assert!(s.borrow().is_none());
            *s.borrow_mut() = Some(Box::new(run));
        });
    }
    pub(super) fn after_private_read_ready() -> Result<()> {
        AFTER_PRIVATE_READ_READY
            .with(|s| s.borrow_mut().take())
            .map_or(Ok(()), |run| run())
    }
    pub(super) fn set_after_final_stage_seal(run: impl FnOnce() -> Result<()> + 'static) {
        AFTER_FINAL_STAGE_SEAL.with(|s| {
            assert!(s.borrow().is_none());
            *s.borrow_mut() = Some(Box::new(run));
        });
    }
    pub(super) fn after_final_stage_seal() -> Result<()> {
        AFTER_FINAL_STAGE_SEAL
            .with(|s| s.borrow_mut().take())
            .map_or(Ok(()), |run| run())
    }
    pub(super) fn set_before_stream_read(run: impl FnOnce() -> Result<()> + 'static) {
        BEFORE_STREAM_READ.with(|s| {
            assert!(s.borrow().is_none());
            *s.borrow_mut() = Some(Box::new(run));
        });
    }
    pub(super) fn before_stream_read() -> Result<()> {
        BEFORE_STREAM_READ
            .with(|s| s.borrow_mut().take())
            .map_or(Ok(()), |run| run())
    }
}

#[cfg(test)]
mod data_quota_tests;
