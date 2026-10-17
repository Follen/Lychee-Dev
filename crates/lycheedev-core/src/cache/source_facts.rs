//! Disposable Source facts and original-byte caches. Authentication belongs to Source;
//! this owner supplies bounded, atomic visibility, not evidence durability.
use crate::{
    Result, ToolkitError, control::OperationControl, metadata::historical_write::finish_historical,
    selection::valid_digest, storage, workspace,
};
use lycheedev_windows::filesystem::destination::{
    CacheReplaceAttempt, Directory, FileControl, FileCost, StagedFile, StagedPublication,
};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    ffi::OsStr,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, Instant},
};

const MAXIMUM: usize = 32 << 20;
/// Optional cumulative wall observations, including failed phase attempts.
/// These subdivide publish's inclusive wall time; they are not CPU timings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceCachePublicationPhase {
    Lock,
    Stage,
    Write,
    Seal,
    Rename,
    StageClose,
    LockClose,
    // Nested within Lock, not additional publication time. KernelAttempt counts
    // every try; ContentionBackoff counts only actual retry sleeps. FailureClose
    // is the consumed acquisition handle on failure, never successful LockClose.
    LockVerify,
    LockOpen,
    LockKernelAttempt,
    LockContentionBackoff,
    LockFailureClose,
}
type PublicationObserver<'a> = Option<&'a mut dyn FnMut(SourceCachePublicationPhase, Duration)>;
fn measured<T>(
    phase: SourceCachePublicationPhase,
    observer: &mut PublicationObserver<'_>,
    run: impl FnOnce() -> T,
) -> T {
    let started = observer.is_some().then(Instant::now);
    let result = run();
    if let (Some(started), Some(observer)) = (started, observer.as_deref_mut()) {
        observer(phase, started.elapsed());
    }
    result
}
struct Controlled<'a>(&'a OperationControl);
impl FileControl for Controlled<'_> {
    type Error = ToolkitError;
    fn charge(&mut self, _: FileCost) -> Result<()> {
        self.check()
    }
    fn check(&mut self) -> Result<()> {
        self.0.check()
    }
}
fn finish_directory<T>(result: Result<T>, directory: Directory) -> Result<T> {
    finish_directory_with(result, directory, Directory::close)
}
fn finish_directory_with<T>(
    mut result: Result<T>,
    directory: Directory,
    close: impl FnOnce(Directory) -> std::result::Result<(), Vec<std::io::Error>>,
) -> Result<T> {
    if let Err(errors) = close(directory) {
        for error in errors {
            result = finish_historical(result, Err(error.into()));
        }
    }
    result
}
/// One authenticated directory per Source invocation. No arbitrary destination,
/// blob/evidence domain, schema knowledge or replacement deadline.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Kind {
    Facts,
    Originals,
}
impl Kind {
    fn directory(self) -> &'static str {
        match self {
            Self::Facts => "source-facts-rust",
            Self::Originals => "source-originals-rust",
        }
    }
}
struct SourceCache {
    directory: Directory,
    locks: PathBuf,
    // Originals only: reads and all Facts operations never acquire this owner.
    // The first Original publication holds locks ancestry until checked finish.
    lock_directory: OnceLock<Directory>,
    kind: Kind,
}
impl SourceCache {
    fn open(home: &Path, kind: Kind, control: &OperationControl) -> Result<Self> {
        workspace::open_controlled(home, control)?;
        let parent =
            Directory::open_shared_controlled(&home.join("cache"), &mut Controlled(control))?;
        let mut directory = None;
        let result = (|| {
            control.check()?;
            let path = parent.path().join(kind.directory());
            match fs::create_dir(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
            control.check()?;
            directory = Some(Directory::open_controlled(&path, &mut Controlled(control))?);
            control.check()
        })();
        let result = finish_directory(result, parent);
        if let Err(error) = result {
            return match directory {
                Some(directory) => finish_directory(Err(error), directory),
                None => Err(error),
            };
        }
        Ok(Self {
            directory: directory.expect("opened cache directory"),
            locks: home.join("locks"),
            lock_directory: OnceLock::new(),
            kind,
        })
    }
    fn name(&self, key: &str) -> Result<String> {
        if !valid_digest(key, 64) {
            return Err(ToolkitError::invalid("invalid Source cache key"));
        }
        Ok(match self.kind {
            Kind::Facts => format!("{key}.json"),
            Kind::Originals => key.to_owned(),
        })
    }
    pub fn read(&self, key: &str, control: &OperationControl) -> Result<Option<Vec<u8>>> {
        let name = self.name(key)?;
        let Some(mut file) = self
            .directory
            .open_optional_project_file_controlled(OsStr::new(&name), &mut Controlled(control))?
        else {
            control.check()?;
            return Ok(None);
        };
        let result = (|| {
            use std::os::windows::fs::MetadataExt;
            control.check()?;
            let info = file.metadata()?;
            if !info.is_file() || info.file_attributes() & 0x400 != 0 {
                return Err(ToolkitError::invalid(
                    "Source cache entry is not a regular file",
                ));
            }
            if info.len() > MAXIMUM as u64 {
                return Ok(None);
            }
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(info.len() as usize).map_err(|_| {
                ToolkitError::new("vault.blob_limit", "body allocation was not admitted", 4)
            })?;
            let mut buffer = [0; 65536];
            while bytes.len() <= MAXIMUM {
                control.check()?;
                let count = (MAXIMUM + 1 - bytes.len()).min(buffer.len());
                let count = file.read(&mut buffer[..count])?;
                if count == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..count]);
            }
            Ok((bytes.len() <= MAXIMUM).then_some(bytes))
        })();
        storage::finish_file(file, result, control)
    }
    pub fn publish(&self, key: &str, bytes: &[u8], control: &OperationControl) -> Result<()> {
        self.publish_observed(key, bytes, control, |_| Ok(()))
    }
    fn lock_directory(&self, control: &OperationControl) -> Result<&Directory> {
        if self.lock_directory.get().is_none() {
            control.check()?;
            fs::create_dir_all(&self.locks)?;
            control.check()?;
            let directory = Directory::open_controlled(&self.locks, &mut Controlled(control))?;
            // Concurrent callers may both acquire valid owners. Close the
            // unused owner once, rather than losing a close failure in Drop.
            if let Err(directory) = self.lock_directory.set(directory) {
                finish_directory(Ok(()), directory)?;
            }
        }
        Ok(self
            .lock_directory
            .get()
            .expect("held Source locks directory"))
    }
    fn publish_observed(
        &self,
        key: &str,
        bytes: &[u8],
        control: &OperationControl,
        mut observe: impl FnMut(&'static str) -> Result<()>,
    ) -> Result<()> {
        self.publish_profiled(key, bytes, control, &mut observe, None)
    }
    fn publish_profiled(
        &self,
        key: &str,
        bytes: &[u8],
        control: &OperationControl,
        observe: &mut impl FnMut(&'static str) -> Result<()>,
        mut timings: PublicationObserver<'_>,
    ) -> Result<()> {
        control.check()?;
        let name = self.name(key)?;
        if bytes.len() > MAXIMUM {
            return Err(match self.kind {
                Kind::Facts => ToolkitError::new(
                    "codebase.fact_budget",
                    "Source fact cache exceeds the 32 MiB limit",
                    3,
                ),
                Kind::Originals => ToolkitError::new("vault.blob_limit", "blob exceeds budget", 4),
            });
        }
        // Facts are rebuildable snapshots: private CREATE owns its slot and
        // same-directory atomic replacement is the publication linearization
        // point. Same-key/different-body callers intentionally use last-writer
        // semantics, without a standalone lock. The normal Published path
        // never reads a later winner; the original NotPublished fallback stays.
        // Originals retain their existing cancellation-aware kernel lock.
        let lock = if self.kind == Kind::Originals {
            let lock_started = timings.is_some().then(Instant::now);
            let lock_result = (|| {
                let directory = self.lock_directory(control)?;
                let resource = format!("source-cache:{}:{key}", self.kind.directory());
                if let Some(observer) = timings.as_deref_mut() {
                    // The synchronous closures borrow one callback; no handle or
                    // error ownership is transferred to this observation adapter.
                    let observer = RefCell::new(observer);
                    storage::ExclusiveLock::resource_wait_in_directory(
                        directory,
                        &resource,
                        control,
                        Some(&|phase, elapsed| {
                            use storage::LockPhase;
                            observer.borrow_mut()(
                                match phase {
                                    LockPhase::Verify => SourceCachePublicationPhase::LockVerify,
                                    LockPhase::Open => SourceCachePublicationPhase::LockOpen,
                                    LockPhase::KernelAttempt => {
                                        SourceCachePublicationPhase::LockKernelAttempt
                                    }
                                    LockPhase::ContentionBackoff => {
                                        SourceCachePublicationPhase::LockContentionBackoff
                                    }
                                    LockPhase::FailureClose => {
                                        SourceCachePublicationPhase::LockFailureClose
                                    }
                                },
                                elapsed,
                            );
                        }),
                    )
                } else {
                    storage::ExclusiveLock::resource_wait_in_directory(
                        directory, &resource, control, None,
                    )
                }
            })();
            if let (Some(started), Some(observer)) = (lock_started, timings.as_deref_mut()) {
                observer(SourceCachePublicationPhase::Lock, started.elapsed());
            }
            Some(lock_result?)
        } else {
            None
        };
        let mut publication = None;
        let result =
            self.publish_staged(key, bytes, control, observe, &mut publication, &mut timings);
        let closed = match lock {
            Some(lock) => measured(SourceCachePublicationPhase::LockClose, &mut timings, || {
                lock.close()
            }),
            None => Ok(()),
        };
        finish_historical(finish_historical(result, closed), control.check()).map_err(|mut error| {
            if let Some((temporary, state)) = publication {
                error.details = serde_json::json!({"schema":"lycheedev.source-cache-publication.v1","path":self.directory.path().join(name),"temporary":temporary,"state":format!("{state:?}"),"durable":false,"cause":error.clone()});
            }
            error
        })
    }
    fn publish_staged(
        &self,
        key: &str,
        bytes: &[u8],
        control: &OperationControl,
        observe: &mut impl FnMut(&'static str) -> Result<()>,
        publication: &mut Option<(String, StagedPublication)>,
        timings: &mut PublicationObserver<'_>,
    ) -> Result<()> {
        let name = self.name(key)?;
        let mut stage = measured(SourceCachePublicationPhase::Stage, timings, ||
            StagedFile::create_rebuildable_cache(&self.directory, key, &mut Controlled(control)))?
            .ok_or_else(|| ToolkitError::new("vault.temporary_collision", "all bounded temporary slots for this target are occupied by active writers or retained entries; no ownership was acquired", 6)
                .with_details(serde_json::json!({"target":self.directory.path().join(&name),"maximumSlots":64,"occupants":"active_or_retained","namespace":format!(".lycheedev-cache-{key}-")})))?;
        let temporary = stage.name().to_owned();
        let result = (|| {
            control.check()?;
            for chunk in bytes.chunks(65536) {
                control.check()?;
                measured(SourceCachePublicationPhase::Write, timings, || {
                    stage.write_all(chunk)
                })?;
                observe("written")?;
            }
            control.check()?;
            measured(SourceCachePublicationPhase::Seal, timings, || {
                stage.seal_rebuildable_cache()
            })?;
            observe("sealed")?;
            control.check()?;
            if measured(SourceCachePublicationPhase::Rename, timings, || {
                if self.kind == Kind::Originals {
                    return stage.replace_name(OsStr::new(&name), &mut Controlled(control));
                }
                loop {
                    match stage.try_replace_rebuildable_cache_name(
                        OsStr::new(&name),
                        &mut Controlled(control),
                    )? {
                        CacheReplaceAttempt::Complete(state) => return Ok(state),
                        CacheReplaceAttempt::SharingBlocked => {
                            // A prior publisher may still be closing its original
                            // deny-delete writer handle. Keep this sealed owner;
                            // never rewrite/reopen it or retry Unknown completion.
                            observe("rename_blocked")?;
                            let pause = control.remaining()?.min(Duration::from_millis(25));
                            std::thread::sleep(pause);
                            control.check()?;
                        }
                    }
                }
            })? == StagedPublication::NotPublished
                && self.read(key, control)?.as_deref() != Some(bytes)
            {
                return Err(ToolkitError::invalid(
                    "Source cache publication winner differs",
                ));
            }
            observe("published")?;
            Ok(())
        })();
        *publication = Some((temporary, stage.publication()));
        let mut result = result;
        for error in measured(SourceCachePublicationPhase::StageClose, timings, || {
            stage.close()
        }) {
            result = finish_historical(result, Err(error.into()));
        }
        result
    }
    pub fn finish<T>(self, result: Result<T>, control: &OperationControl) -> Result<T> {
        self.finish_with(result, control, Directory::close)
    }
    fn finish_with<T>(
        self,
        result: Result<T>,
        control: &OperationControl,
        mut close: impl FnMut(Directory) -> std::result::Result<(), Vec<std::io::Error>>,
    ) -> Result<T> {
        let result = match self.lock_directory.into_inner() {
            Some(directory) => finish_directory_with(result, directory, &mut close),
            None => result,
        };
        finish_historical(
            finish_directory_with(result, self.directory, close),
            control.check(),
        )
    }
}

/// Rebuildable parser results. Source owns the envelope schema and full hash checks.
/// Each publish atomically replaces one complete snapshot, without a standalone
/// lock. Concurrent same-key/different-body publishes are valid: the last rename
/// wins, while an already-open reader retains its original complete snapshot.
/// Success means this candidate was published, or the existing NotPublished
/// fallback validated equivalent bytes, with the owned stage closed. A later
/// writer may replace it. Cancellation/close errors preserve Published/Unknown.
/// Sharing rejection waits under the same absolute control for a preceding
/// writer to close; Unknown completion is never retried. Publication is still
/// synchronous per snapshot (an eager document or one streamed cohort) and does
/// not promise crash durability.
pub struct SourceFactsCache(SourceCache);
/// Rebuildable original bytes, deliberately outside the durable evidence domain.
/// The source index authenticates reads and recovers misses from its fixed Git pin.
pub struct SourceOriginalCache(SourceCache);
macro_rules! source_cache_owner {
    ($owner:ident, $kind:ident) => {
        impl $owner {
            pub fn open(home: &Path, control: &OperationControl) -> Result<Self> {
                SourceCache::open(home, Kind::$kind, control).map(Self)
            }
            pub fn read(&self, key: &str, control: &OperationControl) -> Result<Option<Vec<u8>>> {
                self.0.read(key, control)
            }
            pub fn publish(
                &self,
                key: &str,
                bytes: &[u8],
                control: &OperationControl,
            ) -> Result<()> {
                self.0.publish(key, bytes, control)
            }
            pub fn finish<T>(self, result: Result<T>, control: &OperationControl) -> Result<T> {
                self.0.finish(result, control)
            }
        }
    };
}
source_cache_owner!(SourceFactsCache, Facts);
source_cache_owner!(SourceOriginalCache, Originals);

impl SourceFactsCache {
    /// An owned, bounded reader. Source validates record framing and envelopes;
    /// all reads remain relative to this cache's authenticated directory.
    pub fn open_reader(&self, key: &str, control: &OperationControl) -> Result<Option<SourceFactsReader>> {
        let name = self.0.name(key)?;
        let Some(file) = self.0.directory.open_optional_project_file_controlled(
            OsStr::new(&name), &mut Controlled(control),
        )? else { control.check()?; return Ok(None); };
        let admitted = (|| {
            use std::os::windows::fs::MetadataExt;
            control.check()?;
            let info = file.metadata()?;
            if !info.is_file() || info.file_attributes() & 0x400 != 0 {
                return Err(ToolkitError::invalid("Source cache entry is not a regular file"));
            }
            control.check()?;
            Ok((info.len() <= MAXIMUM as u64).then_some(info.len()))
        })();
        match admitted {
            Ok(Some(bytes)) => Ok(Some(SourceFactsReader { file, bytes })),
            other => storage::finish_file(file, other.map(|_| None), control),
        }
    }
    /// Stream one rebuildable snapshot into a private stage. The caller must
    /// consume this owner with commit or abort, including after a failed write.
    /// No bytes become visible before commit, and no ancestry check is removed.
    pub fn begin_write<'a>(
        &'a self, key: &str, control: &OperationControl,
        mut observer: PublicationObserver<'_>,
    ) -> Result<SourceFactsWriter<'a>> {
        control.check()?;
        let name = self.0.name(key)?;
        let stage = measured(SourceCachePublicationPhase::Stage, &mut observer, ||
            StagedFile::create_rebuildable_cache(&self.0.directory, key, &mut Controlled(control)))?
            .ok_or_else(|| ToolkitError::new("vault.temporary_collision", "all bounded temporary slots for this target are occupied by active writers or retained entries; no ownership was acquired", 6)
                .with_details(serde_json::json!({"target":self.0.directory.path().join(&name),"maximumSlots":64,"occupants":"active_or_retained","namespace":format!(".lycheedev-cache-{key}-")})))?;
        Ok(SourceFactsWriter { cache: &self.0, stage, name, bytes: 0, digest: Sha256::new() })
    }
    /// The ordinary path never reads a clock. Call only when the caller has
    /// explicitly enabled diagnostic phase observations.
    pub fn publish_profiled(
        &self,
        key: &str,
        bytes: &[u8],
        control: &OperationControl,
        observer: &mut dyn FnMut(SourceCachePublicationPhase, Duration),
    ) -> Result<()> {
        self.0
            .publish_profiled(key, bytes, control, &mut |_| Ok(()), Some(observer))
    }
}

/// The exact opened file is retained across lazy record reads and consumed once.
pub struct SourceFactsReader {
    file: fs::File,
    bytes: u64,
}
impl SourceFactsReader {
    pub fn len(&self) -> u64 { self.bytes }
    pub fn is_empty(&self) -> bool { self.bytes == 0 }
    /// Out-of-range or truncated rebuildable data is a miss, never a partial
    /// record. Other I/O and original control errors retain their normal meaning.
    pub fn read_at(&mut self, offset: u64, output: &mut [u8], control: &OperationControl) -> Result<bool> {
        control.check()?;
        if offset.checked_add(output.len() as u64).is_none_or(|end| end > self.bytes) {
            return Ok(false);
        }
        self.file.seek(SeekFrom::Start(offset))?;
        for chunk in output.chunks_mut(65536) {
            control.check()?;
            match self.file.read_exact(chunk) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                    control.check()?; return Ok(false);
                }
                Err(error) => return Err(error.into()),
            }
        }
        control.check()?;
        Ok(true)
    }
    pub fn finish<T>(self, result: Result<T>, control: &OperationControl) -> Result<T> {
        storage::finish_file(self.file, result, control)
    }
}

/// One private, at most 32 MiB, Facts publication. It never owns a second body
/// buffer and cannot be reused after Published or Unknown completion.
pub struct SourceFactsWriter<'a> {
    cache: &'a SourceCache,
    stage: StagedFile<'a>,
    name: String,
    bytes: usize,
    digest: Sha256,
}
impl<'a> SourceFactsWriter<'a> {
    pub fn write(&mut self, bytes: &[u8], control: &OperationControl, mut observer: PublicationObserver<'_>) -> Result<()> {
        control.check()?;
        let next = self.bytes.checked_add(bytes.len()).filter(|n| *n <= MAXIMUM)
            .ok_or_else(|| ToolkitError::new("codebase.fact_budget", "Source fact cache exceeds the 32 MiB limit", 3))?;
        for chunk in bytes.chunks(65536) {
            control.check()?;
            measured(SourceCachePublicationPhase::Write, &mut observer, || self.stage.write_all(chunk))?;
            self.digest.update(chunk);
        }
        self.bytes = next;
        control.check()
    }
    pub fn commit(mut self, control: &OperationControl, mut observer: PublicationObserver<'_>) -> Result<()> {
        let result = (|| {
            control.check()?;
            measured(SourceCachePublicationPhase::Seal, &mut observer, || self.stage.seal_rebuildable_cache())?;
            control.check()?;
            let state = measured(SourceCachePublicationPhase::Rename, &mut observer, || {
                loop {
                    match self.stage.try_replace_rebuildable_cache_name(OsStr::new(&self.name), &mut Controlled(control))? {
                        CacheReplaceAttempt::Complete(state) => return Ok::<_, ToolkitError>(state),
                        CacheReplaceAttempt::SharingBlocked => {
                            let pause = control.remaining()?.min(Duration::from_millis(25));
                            std::thread::sleep(pause);
                            control.check()?;
                        }
                    }
                }
            })?;
            if state == StagedPublication::NotPublished {
                // Preserve the eager path's rare equivalence fallback without
                // materializing the streamed body a second time.
                let Some(mut file) = self.cache.directory.open_optional_project_file_controlled(
                    OsStr::new(&self.name), &mut Controlled(control),
                )? else { return Err(ToolkitError::invalid("Source cache publication winner differs")); };
                let equal = (|| {
                    use std::os::windows::fs::MetadataExt;
                    let info = file.metadata()?;
                    if !info.is_file() || info.file_attributes() & 0x400 != 0 || info.len() != self.bytes as u64 {
                        return Ok(false);
                    }
                    let mut digest = Sha256::new();
                    let mut buffer = [0; 65536];
                    let mut remaining = self.bytes;
                    while remaining != 0 {
                        control.check()?;
                        let count = remaining.min(buffer.len());
                        file.read_exact(&mut buffer[..count])?;
                        digest.update(&buffer[..count]);
                        remaining -= count;
                    }
                    control.check()?;
                    let mut tail = [0];
                    let ended = file.read(&mut tail)? == 0;
                    control.check()?;
                    Ok(ended && digest.finalize() == self.digest.clone().finalize())
                })();
                if !storage::finish_file(file, equal, control)? {
                    return Err(ToolkitError::invalid("Source cache publication winner differs"));
                }
            }
            Ok(())
        })();
        self.abort(result, control, observer)
    }
    /// Close/discard through the original stage owner, preserving primary and
    /// late errors. A published/uncertain stage is never deleted or retried.
    pub fn abort<T>(self, result: Result<T>, control: &OperationControl, observer: PublicationObserver<'_>) -> Result<T> {
        self.abort_with(result, control, observer, StagedFile::close)
    }
    fn abort_with<T>(self, result: Result<T>, control: &OperationControl, mut observer: PublicationObserver<'_>,
        close: impl FnOnce(StagedFile<'a>) -> Vec<std::io::Error>,
    ) -> Result<T> {
        let temporary = self.stage.name().to_owned();
        let state = self.stage.publication();
        let mut result = result;
        for error in measured(SourceCachePublicationPhase::StageClose, &mut observer, || close(self.stage)) {
            result = finish_historical(result, Err(error.into()));
        }
        finish_historical(result, control.check()).map_err(|mut error| {
            // Failure before a commit attempt leaves no public mutation. Keep
            // the caller's original parser/control error unwrapped in that case.
            if state != StagedPublication::NotPublished {
                error.details = serde_json::json!({"schema":"lycheedev.source-cache-publication.v1","path":self.cache.directory.path().join(&self.name),"temporary":temporary,"state":format!("{state:?}"),"durable":false,"cause":error.clone()});
            }
            error
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        sync::Arc,
        sync::Barrier,
        time::{Duration, Instant},
    };
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "lychee-source-cache-{}-{}",
                std::process::id(),
                lycheedev_windows::random::token().unwrap()
            ));
            workspace::initialize(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn publication_profile_subdivides_success_and_preserves_failed_attempts() {
        for fail_after_write in [false, true] {
            let f = Fixture::new();
            let control = OperationControl::default();
            let cache = SourceCache::open(&f.0, Kind::Originals, &control).unwrap();
            let key = storage::sha256(b"profile key");
            let mut phases = Vec::new();
            let result = cache.publish_profiled(
                &key,
                b"complete",
                &control,
                &mut |at| {
                    if fail_after_write && at == "written" {
                        Err(ToolkitError::new("fixture.write", "ordered failure", 5))
                    } else {
                        Ok(())
                    }
                },
                Some(&mut |phase, _| phases.push(phase)),
            );
            use SourceCachePublicationPhase::*;
            if fail_after_write {
                assert_eq!(result.unwrap_err().code, "fixture.write");
                assert_eq!(
                    phases,
                    [
                        LockVerify,
                        LockOpen,
                        LockKernelAttempt,
                        Lock,
                        Stage,
                        Write,
                        StageClose,
                        LockClose
                    ]
                );
                assert!(cache.read(&key, &control).unwrap().is_none());
            } else {
                result.unwrap();
                assert_eq!(
                    phases,
                    [
                        LockVerify,
                        LockOpen,
                        LockKernelAttempt,
                        Lock,
                        Stage,
                        Write,
                        Seal,
                        Rename,
                        StageClose,
                        LockClose
                    ]
                );
                assert_eq!(cache.read(&key, &control).unwrap().unwrap(), b"complete");
            }
            cache.finish(Ok(()), &control).unwrap();
        }
    }
    #[test]
    fn publication_profile_failed_acquire_closes_once_without_a_success_close_phase() {
        let f = Fixture::new();
        let control = OperationControl::default();
        let cancel = control.cancel_handle();
        let cache = SourceCache::open(&f.0, Kind::Originals, &control).unwrap();
        let key = storage::sha256(b"profile stop after kernel lock");
        let mut phases = Vec::new();
        let error = cache
            .publish_profiled(
                &key,
                b"complete",
                &control,
                &mut |_| Ok(()),
                Some(&mut |phase, _| {
                    phases.push(phase);
                    if phase == SourceCachePublicationPhase::LockKernelAttempt {
                        cancel.store(true, std::sync::atomic::Ordering::Release);
                    }
                }),
            )
            .unwrap_err();
        assert_eq!(error.code, "command.cancelled");
        use SourceCachePublicationPhase::*;
        assert_eq!(
            phases,
            [
                LockVerify,
                LockOpen,
                LockKernelAttempt,
                LockFailureClose,
                Lock
            ]
        );
        let next_control = OperationControl::default();
        assert!(cache.read(&key, &next_control).unwrap().is_none());
        // A successful second publisher proves the failed acquisition consumed
        // its original handle and did not leave the kernel lock owned.
        cache.publish(&key, b"complete", &next_control).unwrap();
        cache.finish(Ok(()), &next_control).unwrap();
    }
    #[test]
    fn lock_directory_is_lazy_reused_and_pins_replacement_until_checked_finish() {
        let f = Fixture::new();
        let control = OperationControl::default();
        let cache = SourceCache::open(&f.0, Kind::Originals, &control).unwrap();
        let key = storage::sha256(b"held lock directory");
        assert!(cache.lock_directory.get().is_none());
        assert!(cache.read(&key, &control).unwrap().is_none());
        assert!(cache.lock_directory.get().is_none());
        cache.publish(&key, b"first", &control).unwrap();
        let held = cache.lock_directory.get().unwrap() as *const Directory;
        let moved = f.0.join("moved-locks");
        assert!(fs::rename(&cache.locks, &moved).is_err());
        cache.publish(&key, b"second", &control).unwrap();
        assert_eq!(
            cache.lock_directory.get().unwrap() as *const Directory,
            held
        );
        assert_eq!(cache.read(&key, &control).unwrap().unwrap(), b"second");
        cache.finish(Ok(()), &control).unwrap();
        fs::rename(f.0.join("locks"), &moved).unwrap();
        fs::rename(&moved, f.0.join("locks")).unwrap();
    }
    #[test]
    fn concurrent_first_publications_share_one_held_lock_directory() {
        let f = Fixture::new();
        let control = OperationControl::default();
        let cache = SourceCache::open(&f.0, Kind::Originals, &control).unwrap();
        let barrier = Barrier::new(4);
        std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for ordinal in 0..4 {
                let cache = &cache;
                let barrier = &barrier;
                handles.push(scope.spawn(move || {
                    let control = OperationControl::default();
                    let key = storage::sha256(format!("concurrent-{ordinal}").as_bytes());
                    barrier.wait();
                    cache.publish(&key, b"complete", &control).unwrap();
                    assert_eq!(cache.read(&key, &control).unwrap().unwrap(), b"complete");
                }));
            }
            for handle in handles {
                handle.join().unwrap();
            }
        });
        assert!(cache.lock_directory.get().is_some());
        cache.finish(Ok(()), &control).unwrap();
        let moved = f.0.join("moved-locks");
        fs::rename(f.0.join("locks"), &moved).unwrap();
    }
    #[test]
    fn both_held_directory_closes_preserve_primary_and_late_control_causes() {
        let f = Fixture::new();
        let control = OperationControl::default();
        let cache = SourceCache::open(&f.0, Kind::Originals, &control).unwrap();
        cache
            .publish(&storage::sha256(b"close"), b"complete", &control)
            .unwrap();
        let token = control.cancel_handle();
        let mut closes = 0;
        let result: Result<()> = Err(ToolkitError::new("fixture.primary", "first failure", 5));
        let error = cache
            .finish_with(result, &control, |directory| {
                closes += 1;
                let mut errors = directory.close().err().unwrap_or_default();
                errors.push(std::io::Error::other(format!(
                    "injected-directory-close-{closes}"
                )));
                token.store(true, std::sync::atomic::Ordering::Release);
                Err(errors)
            })
            .unwrap_err();
        assert_eq!(closes, 2);
        assert_eq!(error.code, "fixture.primary");
        let causes = error.details.to_string();
        for cause in [
            "fixture.primary",
            "injected-directory-close-1",
            "injected-directory-close-2",
            "command.cancelled",
        ] {
            assert!(causes.contains(cause), "missing original cause: {cause}");
        }
        fs::rename(f.0.join("locks"), f.0.join("released-locks")).unwrap();
    }
    #[test]
    fn fixed_domains_reject_paths_and_preserve_distinct_budget_errors() {
        let f = Fixture::new();
        let control = OperationControl::default();
        let facts = SourceCache::open(&f.0, Kind::Facts, &control).unwrap();
        let originals = SourceCache::open(&f.0, Kind::Originals, &control).unwrap();
        for key in [
            "../escape",
            "",
            &"f".repeat(63),
            &format!("{}:stream", "f".repeat(64)),
        ] {
            assert!(facts.publish(key, b"x", &control).is_err());
            assert!(originals.read(key, &control).is_err());
        }
        let large = vec![0; MAXIMUM + 1];
        let key = storage::sha256(b"x");
        assert_eq!(
            facts.publish(&key, &large, &control).unwrap_err().code,
            "codebase.fact_budget"
        );
        assert_eq!(
            originals.publish(&key, &large, &control).unwrap_err().code,
            "vault.blob_limit"
        );
        facts.finish(Ok(()), &control).unwrap();
        originals.finish(Ok(()), &control).unwrap();
        assert!(!f.0.join("blobs").join(&key[..2]).join(&key[2..]).exists());
    }
    #[test]
    fn partial_write_and_prepublication_stop_preserve_complete_old_entry() {
        for stopped_at in ["written", "sealed"] {
            let f = Fixture::new();
            let control = OperationControl::default();
            let cache = SourceCache::open(&f.0, Kind::Facts, &control).unwrap();
            let key = storage::sha256(b"fact identity");
            cache.publish(&key, b"old complete", &control).unwrap();
            let body = vec![42; 131073];
            let error = cache
                .publish_observed(&key, &body, &control, |at| {
                    if at == stopped_at {
                        Err(ToolkitError::new(
                            "test.write_failed",
                            "injected failure",
                            5,
                        ))
                    } else {
                        Ok(())
                    }
                })
                .unwrap_err();
            assert_eq!(error.code, "test.write_failed");
            assert_eq!(error.details["state"], "NotPublished");
            assert_eq!(
                cache.read(&key, &control).unwrap().unwrap(),
                b"old complete"
            );
            assert_eq!(fs::read_dir(cache.directory.path()).unwrap().count(), 1);
            cache.finish(Ok(()), &control).unwrap();
        }
    }
    #[test]
    fn cancellation_before_publish_discards_only_private_stage() {
        let f = Fixture::new();
        let control = OperationControl::default();
        let cache = SourceCache::open(&f.0, Kind::Originals, &control).unwrap();
        let key = storage::sha256(b"full body");
        let cancelled = OperationControl::default();
        let token = cancelled.cancel_handle();
        let error = cache
            .publish_observed(&key, b"full body", &cancelled, |at| {
                if at == "sealed" {
                    token.store(true, std::sync::atomic::Ordering::Release);
                }
                Ok(())
            })
            .unwrap_err();
        assert_eq!(error.code, "command.cancelled");
        assert_eq!(error.details["state"], "NotPublished");
        assert!(cache.read(&key, &control).unwrap().is_none());
        assert_eq!(fs::read_dir(cache.directory.path()).unwrap().count(), 0);
        cache.finish(Ok(()), &control).unwrap();
    }
    #[test]
    fn late_cancellation_reports_published_cache_without_retry_or_deletion() {
        let f = Fixture::new();
        let control = OperationControl::default();
        let cache = SourceCache::open(&f.0, Kind::Facts, &control).unwrap();
        let key = storage::sha256(b"key");
        let cancelled = OperationControl::default();
        let token = cancelled.cancel_handle();
        let error = cache
            .publish_observed(&key, b"complete", &cancelled, |at| {
                if at == "published" {
                    token.store(true, std::sync::atomic::Ordering::Release);
                }
                Ok(())
            })
            .unwrap_err();
        assert_eq!(error.code, "command.cancelled");
        assert_eq!(error.details["state"], "Published");
        assert_eq!(cache.read(&key, &control).unwrap().unwrap(), b"complete");
        cache.finish(Ok(()), &control).unwrap();
    }
    #[test]
    fn replacement_failure_preserves_destination_and_reports_uncertain_stage() {
        let f = Fixture::new();
        let control = OperationControl::default();
        let cache = SourceCache::open(&f.0, Kind::Facts, &control).unwrap();
        let key = storage::sha256(b"blocked");
        let name = cache.name(&key).unwrap();
        fs::create_dir(cache.directory.path().join(&name)).unwrap();
        let error = cache.publish(&key, b"complete", &control).unwrap_err();
        assert!(cache.directory.path().join(name).is_dir());
        assert!(matches!(
            error.details["state"].as_str(),
            Some("Unknown" | "NotPublished")
        ));
        cache.finish(Ok(()), &control).unwrap();
    }
    #[test]
    fn concurrent_directory_owners_publish_complete_winners() {
        let f = Fixture::new();
        let key = storage::sha256(b"same immutable identity");
        let barrier = Arc::new(Barrier::new(3));
        std::thread::scope(|scope| {
            let mut writers = Vec::new();
            for fill in [17u8, 29u8] {
                let barrier = barrier.clone();
                let key = &key;
                let home = &f.0;
                writers.push(scope.spawn(move || {
                    let control = OperationControl::default();
                    let opened = SourceCache::open(home, Kind::Facts, &control);
                    // Even an acquisition failure reaches the barrier, so this
                    // regression reports an error rather than hanging peers.
                    barrier.wait();
                    let cache = opened.unwrap();
                    let bytes = vec![fill; 131073];
                    for _ in 0..8 {
                        cache.publish(key, &bytes, &control).unwrap();
                    }
                    cache.finish(Ok(()), &control).unwrap();
                }));
            }
            let control = OperationControl::default();
            let reader = SourceCache::open(&f.0, Kind::Facts, &control);
            barrier.wait();
            let reader = reader.unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while writers.iter().any(|writer| !writer.is_finished()) {
                assert!(Instant::now() < deadline, "cache publication stalled");
                if let Some(bytes) = reader.read(&key, &control).unwrap() {
                    assert_eq!(bytes.len(), 131073);
                    assert!(bytes.iter().all(|&b| b == 17) || bytes.iter().all(|&b| b == 29));
                }
            }
            for writer in writers {
                writer.join().unwrap();
            }
            assert!(reader.read(&key, &control).unwrap().is_some());
            reader.finish(Ok(()), &control).unwrap();
        });
    }
    #[test]
    fn original_and_facts_domains_never_alias_evidence_or_each_other() {
        let f = Fixture::new();
        let control = OperationControl::default();
        let key = storage::sha256(b"body");
        let facts = SourceFactsCache::open(&f.0, &control).unwrap();
        let originals = SourceOriginalCache::open(&f.0, &control).unwrap();
        facts.publish(&key, b"facts envelope", &control).unwrap();
        originals.publish(&key, b"body", &control).unwrap();
        facts.finish(Ok(()), &control).unwrap();
        originals.finish(Ok(()), &control).unwrap();
        assert!(!crate::evidence::blob_path(&f.0, &key).unwrap().exists());
        let blob = crate::evidence::publish_blob_bounded(&f.0, b"body", MAXIMUM, &control).unwrap();
        fs::remove_dir_all(f.0.join("cache/source-originals-rust")).unwrap();
        assert_eq!(
            crate::evidence::read_blob_bounded(&f.0, &blob, MAXIMUM, &control).unwrap(),
            b"body"
        );
    }
    #[test]
    fn same_key_wait_respects_deadline_and_never_releases_other_owner() {
        let f = Fixture::new();
        let control = OperationControl::default();
        let cache = SourceCache::open(&f.0, Kind::Originals, &control).unwrap();
        let key = storage::sha256(b"busy key");
        let resource = format!("source-cache:{}:{key}", Kind::Originals.directory());
        let owner =
            storage::ExclusiveLock::resource_controlled(&cache.locks, &resource, &control).unwrap();
        let short = OperationControl::uncancelable(Instant::now() + Duration::from_millis(30));
        assert_eq!(
            cache.publish(&key, b"complete", &short).unwrap_err().code,
            "command.deadline"
        );
        assert!(cache.read(&key, &control).unwrap().is_none());
        let still_busy =
            storage::ExclusiveLock::resource_controlled(&cache.locks, &resource, &control)
                .err()
                .unwrap();
        assert_eq!(still_busy.code, "workspace.busy");
        owner.close().unwrap();
        cache.publish(&key, b"complete", &control).unwrap();
        cache.finish(Ok(()), &control).unwrap();
    }
    #[test]
    fn bounded_uncertain_slots_are_never_adopted_or_removed() {
        let f = Fixture::new();
        let control = OperationControl::default();
        let cache = SourceCache::open(&f.0, Kind::Facts, &control).unwrap();
        let key = storage::sha256(b"blocked slots");
        for slot in 0..64 {
            fs::write(
                cache
                    .directory
                    .path()
                    .join(format!(".lycheedev-cache-{key}-{slot}")),
                format!("foreign-{slot}"),
            )
            .unwrap();
        }
        let error = cache.publish(&key, b"complete", &control).unwrap_err();
        assert_eq!(error.code, "vault.temporary_collision");
        assert_eq!(error.details["maximumSlots"], 64);
        assert_eq!(error.details["occupants"], "active_or_retained");
        for slot in 0..64 {
            assert_eq!(
                fs::read(
                    cache
                        .directory
                        .path()
                        .join(format!(".lycheedev-cache-{key}-{slot}"))
                )
                .unwrap(),
                format!("foreign-{slot}").as_bytes()
            );
        }
        assert_eq!(fs::read_dir(cache.directory.path()).unwrap().count(), 64);
        // A saturated target does not reserve or block the whole directory.
        cache
            .publish(&storage::sha256(b"other target"), b"complete", &control)
            .unwrap();
        cache.finish(Ok(()), &control).unwrap();
    }
}

#[cfg(test)]
mod atomic_publication_tests;

#[cfg(test)]
mod streaming_tests {
    use super::*;
    use std::sync::{Arc, Barrier, atomic::{AtomicBool, Ordering}};
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let home = std::env::temp_dir().join(format!("lychee-stream-facts-{}", lycheedev_windows::random::token().unwrap()));
            workspace::initialize(&home).unwrap(); Self(home)
        }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    #[test]
    fn streamed_facts_are_invisible_until_atomic_commit_and_range_read_is_bounded() {
        let fixture = Fixture::new(); let control = OperationControl::default();
        let cache = SourceFactsCache::open(&fixture.0, &control).unwrap();
        let key = storage::sha256(b"streamed");
        let mut writer = cache.begin_write(&key, &control, None).unwrap();
        writer.write(b"first", &control, None).unwrap();
        assert!(cache.read(&key, &control).unwrap().is_none());
        writer.write(b"second", &control, None).unwrap(); writer.commit(&control, None).unwrap();
        let mut reader = cache.open_reader(&key, &control).unwrap().unwrap();
        assert_eq!(reader.len(), 11);
        let mut range = [0; 6]; assert!(reader.read_at(5, &mut range, &control).unwrap()); assert_eq!(&range, b"second");
        assert!(!reader.read_at(u64::MAX, &mut range, &control).unwrap());
        assert!(!reader.read_at(6, &mut range, &control).unwrap());
        reader.finish(Ok(()), &control).unwrap(); cache.finish(Ok(()), &control).unwrap();
    }
    #[test]
    fn streamed_same_key_publishers_keep_complete_winners_and_old_readers() {
        let fixture = Fixture::new(); let control = OperationControl::default();
        let cache = SourceFactsCache::open(&fixture.0, &control).unwrap(); let key = storage::sha256(b"shared");
        cache.publish(&key, b"old", &control).unwrap();
        let mut old = cache.open_reader(&key, &control).unwrap().unwrap();
        let barrier = Barrier::new(4);
        std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for value in b'a'..=b'd' {
                let (cache, key, control, barrier) = (&cache, &key, &control, &barrier);
                handles.push(scope.spawn(move || {
                    let prepared = cache.begin_write(key, control, None).and_then(|mut writer| {
                        writer.write(&vec![value; 65539], control, None)?; Ok(writer)
                    });
                    // All peers reach the barrier even if acquisition failed.
                    barrier.wait(); prepared.unwrap().commit(control, None).unwrap();
                }));
            }
            for handle in handles { handle.join().unwrap(); }
        });
        let winner = cache.read(&key, &control).unwrap().unwrap();
        assert_eq!(winner.len(), 65539); assert!(winner.iter().all(|b| *b == winner[0])); assert!((b'a'..=b'd').contains(&winner[0]));
        let mut bytes = [0; 3]; assert!(old.read_at(0, &mut bytes, &control).unwrap()); assert_eq!(&bytes, b"old");
        old.finish(Ok(()), &control).unwrap(); cache.finish(Ok(()), &control).unwrap();
        assert_eq!(fs::read_dir(fixture.0.join("cache/source-facts-rust")).unwrap().count(), 1);
    }
    #[test]
    fn streamed_size_and_cancellation_errors_abort_private_stage_without_losing_primary() {
        let fixture = Fixture::new(); let control = OperationControl::default();
        let cache = SourceFactsCache::open(&fixture.0, &control).unwrap(); let key = storage::sha256(b"limit");
        let mut writer = cache.begin_write(&key, &control, None).unwrap(); writer.write(b"one", &control, None).unwrap();
        let body = vec![0; MAXIMUM - 3];
        writer.write(&body, &control, None).unwrap();
        let error = writer.write(b"x", &control, None).unwrap_err(); assert_eq!(error.code, "codebase.fact_budget");
        assert_eq!(writer.abort::<()>(Err(error), &control, None).unwrap_err().code, "codebase.fact_budget");
        assert!(cache.read(&key, &control).unwrap().is_none());
        let mut writer = cache.begin_write(&key, &control, None).unwrap(); writer.write(b"private", &control, None).unwrap();
        let stopped = OperationControl::new(Instant::now()+Duration::from_secs(60), Arc::new(AtomicBool::new(true)));
        let error = writer.write(b"later", &stopped, None).unwrap_err();
        assert_eq!(writer.abort::<()>(Err(error), &stopped, None).unwrap_err().code, "command.cancelled");
        cache.finish(Ok(()), &control).unwrap();
        assert_eq!(fs::read_dir(fixture.0.join("cache/source-facts-rust")).unwrap().count(), 0);
    }
    #[test]
    fn streamed_late_stop_retains_published_identity_and_checked_close() {
        let fixture = Fixture::new(); let token = Arc::new(AtomicBool::new(false));
        let control = OperationControl::new(Instant::now()+Duration::from_secs(60), token.clone());
        let cache = SourceFactsCache::open(&fixture.0, &control).unwrap(); let key = storage::sha256(b"late stop");
        let mut writer = cache.begin_write(&key, &control, None).unwrap(); writer.write(b"committed", &control, None).unwrap();
        let mut closes = 0;
        let error = writer.commit(&control, Some(&mut |phase, _| {
            if phase == SourceCachePublicationPhase::Rename { token.store(true, Ordering::Release); }
            if phase == SourceCachePublicationPhase::StageClose { closes += 1; }
        })).unwrap_err();
        assert_eq!(error.code, "command.cancelled"); assert_eq!(error.details["state"], "Published"); assert_eq!(closes, 1);
        cache.finish::<()>(Err(error), &control).unwrap_err();
        let live = OperationControl::default(); let next = SourceFactsCache::open(&fixture.0, &live).unwrap();
        assert_eq!(next.read(&key, &live).unwrap().unwrap(), b"committed"); next.finish(Ok(()), &live).unwrap();
    }

    #[test]
    fn streamed_partial_write_stop_preserves_old_snapshot_and_foreign_stage() {
        let fixture = Fixture::new(); let live = OperationControl::default();
        let cache = SourceFactsCache::open(&fixture.0, &live).unwrap();
        let key = storage::sha256(b"partial stop"); cache.publish(&key, b"old", &live).unwrap();
        let foreign = fixture.0.join("cache/source-facts-rust").join(format!(".lycheedev-cache-{key}-0"));
        fs::write(&foreign, b"unowned").unwrap();
        let control = OperationControl::default(); let token = control.cancel_handle();
        let mut writer = cache.begin_write(&key, &control, None).unwrap();
        let mut writes = 0;
        let error = writer.write(&vec![11; 131073], &control, Some(&mut |phase, _| {
            if phase == SourceCachePublicationPhase::Write { writes += 1; token.store(true, Ordering::Release); }
        })).unwrap_err();
        assert_eq!(writes, 1); assert_eq!(error.code, "command.cancelled");
        assert_eq!(writer.abort::<()>(Err(error), &control, None).unwrap_err().code, "command.cancelled");
        assert_eq!(cache.read(&key, &live).unwrap().unwrap(), b"old");
        assert_eq!(fs::read(foreign).unwrap(), b"unowned");
        assert_eq!(fs::read_dir(fixture.0.join("cache/source-facts-rust")).unwrap().count(), 2);
        cache.finish(Ok(()), &live).unwrap();
    }
    #[test]
    fn streamed_abort_keeps_primary_close_and_final_control_errors() {
        let fixture = Fixture::new(); let control = OperationControl::default();
        let cache = SourceFactsCache::open(&fixture.0, &control).unwrap();
        let key = storage::sha256(b"close causes"); let token = control.cancel_handle();
        let mut writer = cache.begin_write(&key, &control, None).unwrap(); writer.write(b"private", &control, None).unwrap();
        let mut closes = 0;
        let error = writer.abort_with::<()>(Err(ToolkitError::new("fixture.primary", "earlier error", 5)), &control, None, |stage| {
            closes += 1;
            let mut errors = stage.close();
            errors.push(std::io::Error::other("fixture.streamed_close"));
            token.store(true, Ordering::Release); errors
        }).unwrap_err();
        assert_eq!(closes, 1); assert_eq!(error.code, "fixture.primary");
        for cause in ["fixture.primary", "fixture.streamed_close", "command.cancelled"] {
            assert!(serde_json::to_string(&error).unwrap().contains(cause), "{cause}");
        }
        cache.finish::<()>(Err(error), &control).unwrap_err();
        assert_eq!(fs::read_dir(fixture.0.join("cache/source-facts-rust")).unwrap().count(), 0);
    }
    #[test]
    fn streamed_replace_failure_preserves_terminal_state_and_original_destination() {
        let fixture = Fixture::new(); let control = OperationControl::default();
        let cache = SourceFactsCache::open(&fixture.0, &control).unwrap();
        let key = storage::sha256(b"unknown replacement");
        let destination = fixture.0.join("cache/source-facts-rust").join(format!("{key}.json"));
        fs::create_dir(&destination).unwrap();
        let mut writer = cache.begin_write(&key, &control, None).unwrap();
        let temporary = writer.stage.name().to_owned(); writer.write(b"complete private", &control, None).unwrap();
        let mut renames = 0; let mut closes = 0;
        let error = writer.commit(&control, Some(&mut |phase, _| {
            if phase == SourceCachePublicationPhase::Rename { renames += 1; }
            if phase == SourceCachePublicationPhase::StageClose { closes += 1; }
        })).unwrap_err();
        assert_eq!((renames, closes), (1, 1)); assert!(destination.is_dir());
        let temporary = destination.parent().unwrap().join(temporary);
        match error.details["state"].as_str() {
            Some("Unknown") => assert_eq!(fs::read(temporary).unwrap(), b"complete private"),
            // A filesystem may report the directory collision as AlreadyExists;
            // that precise rejection permits discard of the still-private stage.
            _ => { assert!(error.details.is_null()); assert!(!temporary.exists()); }
        }
        cache.finish(Ok(()), &control).unwrap();
    }
    #[test]
    fn streamed_unknown_stage_is_not_retried_or_discarded() {
        let fixture = Fixture::new(); let control = OperationControl::default();
        let cache = SourceFactsCache::open(&fixture.0, &control).unwrap();
        let key = storage::sha256(b"uncertain owned stage"); let name = format!("{key}.json");
        let mut previous = StagedFile::create_rebuildable_cache(&cache.0.directory, &key, &mut Controlled(&control)).unwrap().unwrap();
        previous.write_all(b"first complete").unwrap(); previous.seal_rebuildable_cache().unwrap();
        assert_eq!(previous.replace_name(OsStr::new(&name), &mut Controlled(&control)).unwrap(), StagedPublication::Published);
        let mut writer = cache.begin_write(&key, &control, None).unwrap();
        let temporary = writer.stage.name().to_owned(); writer.write(b"second private", &control, None).unwrap();
        writer.stage.seal_rebuildable_cache().unwrap();
        // The ordinary replacement API deliberately cannot classify a generic
        // sharing error as the cache-specific native SharingBlocked rejection.
        // The held deny-delete writer makes that error real and deterministic.
        assert!(writer.stage.replace_name(OsStr::new(&name), &mut Controlled(&control)).is_err());
        assert_eq!(writer.stage.publication(), StagedPublication::Unknown);
        let mut renames = 0; let mut closes = 0;
        let error = writer.commit(&control, Some(&mut |phase, _| {
            if phase == SourceCachePublicationPhase::Rename { renames += 1; }
            if phase == SourceCachePublicationPhase::StageClose { closes += 1; }
        })).unwrap_err();
        assert_eq!(error.details["state"], "Unknown"); assert_eq!((renames, closes), (0, 1));
        assert!(previous.close().is_empty());
        assert_eq!(cache.read(&key, &control).unwrap().unwrap(), b"first complete");
        assert_eq!(fs::read(cache.0.directory.path().join(temporary)).unwrap(), b"second private");
        cache.finish(Ok(()), &control).unwrap();
    }
    struct ProcessOwner(Option<std::process::Child>);
    impl ProcessOwner {
        fn start(home: &Path, key: &str, id: &str, value: u8) -> Self {
            Self(Some(std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "cache::source_facts::streaming_tests::streamed_publication_process_helper", "--nocapture"])
                .env("LYCHEEDEV_STREAMED_FACTS_HOME", home).env("LYCHEEDEV_STREAMED_FACTS_KEY", key)
                .env("LYCHEEDEV_STREAMED_FACTS_ID", id).env("LYCHEEDEV_STREAMED_FACTS_BYTE", value.to_string())
                .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::inherit())
                .spawn().unwrap()))
        }
        fn ready(&mut self, home: &Path, id: &str, deadline: Instant) {
            while !home.join(format!("{id}.ready")).try_exists().unwrap() {
                assert!(self.0.as_mut().unwrap().try_wait().unwrap().is_none(), "stream helper exited early");
                assert!(Instant::now() < deadline, "stream helper readiness deadline");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        fn finish(&mut self, deadline: Instant) {
            while self.0.as_mut().unwrap().try_wait().unwrap().is_none() {
                assert!(Instant::now() < deadline, "stream helper publication deadline");
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(self.0.as_mut().unwrap().wait().unwrap().success()); self.0.take();
        }
    }
    impl Drop for ProcessOwner {
        fn drop(&mut self) {
            if let Some(mut child) = self.0.take() {
                if !matches!(child.try_wait(), Ok(Some(_))) { let _ = child.kill(); }
                let _ = child.wait();
            }
        }
    }
    #[test]
    fn streamed_publication_process_helper() {
        let Some(home) = std::env::var_os("LYCHEEDEV_STREAMED_FACTS_HOME") else { return; };
        let home = PathBuf::from(home); let key = std::env::var("LYCHEEDEV_STREAMED_FACTS_KEY").unwrap();
        let id = std::env::var("LYCHEEDEV_STREAMED_FACTS_ID").unwrap();
        let value: u8 = std::env::var("LYCHEEDEV_STREAMED_FACTS_BYTE").unwrap().parse().unwrap();
        let control = OperationControl::uncancelable(Instant::now()+Duration::from_secs(30));
        let cache = SourceFactsCache::open(&home, &control).unwrap();
        let mut writer = cache.begin_write(&key, &control, None).unwrap();
        let prepared = (|| {
            writer.write(&vec![value; 131073], &control, None)?;
            fs::write(home.join(format!("{id}.ready")), b"private complete")?;
            while !home.join(format!("{id}.go")).try_exists()? { control.check()?; std::thread::sleep(Duration::from_millis(5)); }
            Ok(())
        })();
        let result = match prepared { Ok(()) => writer.commit(&control, None), Err(error) => writer.abort(Err(error), &control, None) };
        cache.finish(result, &control).unwrap();
    }
    #[test]
    fn streamed_two_processes_hold_distinct_stages_and_preserve_old_open_reader() {
        let fixture = Fixture::new(); let control = OperationControl::default();
        let cache = SourceFactsCache::open(&fixture.0, &control).unwrap();
        let key = storage::sha256(b"two streamed processes"); cache.publish(&key, b"old", &control).unwrap();
        let mut old = cache.open_reader(&key, &control).unwrap().unwrap();
        let deadline = Instant::now()+Duration::from_secs(20);
        let mut first = ProcessOwner::start(&fixture.0, &key, "first", 17);
        let mut second = ProcessOwner::start(&fixture.0, &key, "second", 29);
        first.ready(&fixture.0, "first", deadline); second.ready(&fixture.0, "second", deadline);
        assert_eq!(fs::read_dir(fixture.0.join("cache/source-facts-rust")).unwrap().count(), 3);
        assert_eq!(cache.read(&key, &control).unwrap().unwrap(), b"old");
        fs::write(fixture.0.join("first.go"), b"commit").unwrap(); first.finish(deadline);
        assert_eq!(cache.read(&key, &control).unwrap().unwrap(), vec![17; 131073]);
        fs::write(fixture.0.join("second.go"), b"commit").unwrap(); second.finish(deadline);
        assert_eq!(cache.read(&key, &control).unwrap().unwrap(), vec![29; 131073]);
        let mut raw = [0; 3]; assert!(old.read_at(0, &mut raw, &control).unwrap()); assert_eq!(&raw, b"old");
        old.finish(Ok(()), &control).unwrap(); cache.finish(Ok(()), &control).unwrap();
        assert_eq!(fs::read_dir(fixture.0.join("cache/source-facts-rust")).unwrap().count(), 1);
    }
}
