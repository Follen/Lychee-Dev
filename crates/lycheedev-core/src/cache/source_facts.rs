//! Disposable Source facts and original-byte caches. Authentication belongs to Source;
//! this owner supplies bounded, atomic visibility, not evidence durability.
use crate::{
    Result, ToolkitError, control::OperationControl, metadata::historical_write::finish_historical,
    selection::valid_digest, storage, workspace,
};
use lycheedev_windows::filesystem::destination::{
    Directory, FileControl, FileCost, StagedFile, StagedPublication,
};
use std::{
    ffi::OsStr,
    fs,
    io::Read,
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
#[derive(Clone, Copy)]
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
    // Lazy: reads retain their original no-lock-publication path. A successful
    // first publication holds the real locks ancestry until checked finish.
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
        // Same-key publishers serialize through the existing bounded,
        // cancellation-aware lock primitive. Different files/directories do
        // not contend; readers keep old complete bytes across replacement.
        let lock = measured(SourceCachePublicationPhase::Lock, &mut timings, || {
            storage::ExclusiveLock::resource_wait_in_directory(
                self.lock_directory(control)?,
                &format!("source-cache:{}:{key}", self.kind.directory()),
                control,
            )
        })?;
        let mut publication = None;
        let result =
            self.publish_locked(key, bytes, control, observe, &mut publication, &mut timings);
        let closed = measured(SourceCachePublicationPhase::LockClose, &mut timings, || {
            lock.close()
        });
        finish_historical(finish_historical(result, closed), control.check()).map_err(|mut error| {
            if let Some((temporary, state)) = publication {
                error.details = serde_json::json!({"schema":"lycheedev.source-cache-publication.v1","path":self.directory.path().join(name),"temporary":temporary,"state":format!("{state:?}"),"durable":false,"cause":error.clone()});
            }
            error
        })
    }
    fn publish_locked(
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
            .ok_or_else(|| ToolkitError::new("vault.temporary_collision", "all bounded temporary slots for this target are occupied; preserve and inspect uncertain residues", 6)
                .with_details(serde_json::json!({"target":self.directory.path().join(&name),"maximumSlots":64,"namespace":format!(".lycheedev-cache-{key}-")})))?;
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
                stage.replace_name(OsStr::new(&name), &mut Controlled(control))
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
            let cache = SourceCache::open(&f.0, Kind::Facts, &control).unwrap();
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
                assert_eq!(phases, [Lock, Stage, Write, StageClose, LockClose]);
                assert!(cache.read(&key, &control).unwrap().is_none());
            } else {
                result.unwrap();
                assert_eq!(
                    phases,
                    [Lock, Stage, Write, Seal, Rename, StageClose, LockClose]
                );
                assert_eq!(cache.read(&key, &control).unwrap().unwrap(), b"complete");
            }
            cache.finish(Ok(()), &control).unwrap();
        }
    }
    #[test]
    fn lock_directory_is_lazy_reused_and_pins_replacement_until_checked_finish() {
        let f = Fixture::new();
        let control = OperationControl::default();
        let cache = SourceCache::open(&f.0, Kind::Facts, &control).unwrap();
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
        let cache = SourceCache::open(&f.0, Kind::Facts, &control).unwrap();
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
        let cache = SourceCache::open(&f.0, Kind::Facts, &control).unwrap();
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
        let cache = SourceCache::open(&f.0, Kind::Facts, &control).unwrap();
        let key = storage::sha256(b"busy key");
        let resource = format!("source-cache:{}:{key}", Kind::Facts.directory());
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
