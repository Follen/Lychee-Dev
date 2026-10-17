//! Safe, identity-bound private spool using the existing same-directory Stage.
//! Source/business verification remains the caller's responsibility.
use super::*;
use std::io::{Seek, SeekFrom};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StagedPublication {
    NotPublished,
    Published,
    Unknown,
}

/// One rebuildable-cache replace attempt. SharingBlocked is a definite native
/// rejection, not Unknown and not an existing-winner success. The sealed private
/// owner remains held; a caller may retry it under its original bounded control.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheReplaceAttempt {
    Complete(StagedPublication),
    SharingBlocked,
}

/// Borrows its original strict Directory and never exposes a writable File.
/// A sealed spool may be reread through the same handle but never written again.
pub struct StagedFile<'a> {
    stage: Stage,
    parent: &'a Directory,
    publication: StagedPublication,
    sealed: bool,
    rebuildable_cache: bool,
}
impl<'a> StagedFile<'a> {
    /// The caller checks again immediately after receiving this owner, so a
    /// post-acquisition stop still has identity-bound discard/close authority.
    pub fn create<C: FileControl>(
        parent: &'a Directory,
        control: &mut C,
    ) -> Result<Self, C::Error> {
        parent.verify_controlled(control)?;
        let stage = Stage::create_verified(parent, control)?;
        Ok(Self {
            stage,
            parent,
            publication: StagedPublication::NotPublished,
            sealed: false,
            rebuildable_cache: false,
        })
    }
    pub fn name(&self) -> &str {
        &self.stage.name
    }
    /// A disposable-cache target has at most 64 private stage names, shared by
    /// concurrent active writers and retained uncertain/foreign entries. Names
    /// are fixed by one lowercase SHA-256 key, never caller paths. Exclusive
    /// CREATE alone acquires ownership; callers need not serialize same-key
    /// publication. An occupied slot is skipped, never opened or removed.
    /// None means this bounded pass found no free slot, not that all occupants
    /// are abandoned. No liveness, age or PID inference authorizes reclamation.
    /// The caller owns the returned handle before its next cancellation check.
    pub fn create_rebuildable_cache<C: FileControl>(
        parent: &'a Directory,
        key: &str,
        control: &mut C,
    ) -> Result<Option<Self>, C::Error> {
        if key.len() != 64
            || !key
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid("cache stage requires one lowercase SHA-256 key").into());
        }
        parent.verify_controlled(control)?;
        for slot in 0..64 {
            control.charge(FileCost {
                io_operations: 4,
                allocation_bytes: 4096,
                work_units: 4096,
            })?;
            control.check()?;
            let name = format!(".lycheedev-cache-{key}-{slot}");
            match relative_file(
                parent.parent_handle(),
                std::ffi::OsStr::new(&name),
                RelativeOpen::Stage,
            ) {
                Ok(file) => {
                    return Ok(Some(Self {
                        stage: Stage {
                            file: Some(file),
                            name,
                            published: false,
                        },
                        parent,
                        publication: StagedPublication::NotPublished,
                        sealed: false,
                        rebuildable_cache: true,
                    }));
                }
                // FILE_CREATE can report a sharing violation for a live stage
                // whose original owner denies write/delete sharing. This is
                // occupancy, just like a name collision; it grants no handle
                // or cleanup authority. Do not swallow other access failures.
                Err(error)
                    if error.kind() == io::ErrorKind::AlreadyExists
                        || error.raw_os_error() == Some(32) =>
                {
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
        }
        control.check()?;
        Ok(None)
    }
    pub fn publication(&self) -> StagedPublication {
        self.publication
    }
    fn require_private(&self) -> io::Result<()> {
        if self.publication != StagedPublication::NotPublished {
            Err(invalid("a committed or uncertain stage cannot be reused"))
        } else {
            Ok(())
        }
    }
    pub fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.require_private()?;
        if self.sealed {
            return Err(invalid("a sealed spool cannot be written"));
        }
        self.stage.file_mut().write_all(bytes)
    }
    /// Random access is limited to the same private unsealed output owner.
    /// ZIP finalization may patch its local header before seal; neither sealed
    /// readers nor committed/uncertain files gain mutable access.
    pub fn seek_private(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.require_private()?;
        if self.sealed {
            return Err(invalid("a sealed spool cannot be repositioned for writing"));
        }
        self.stage.file_mut().seek(position)
    }
    /// Sync and rewind the original held handle, not a temporary pathname.
    pub fn seal(&mut self) -> io::Result<()> {
        self.seal_with(File::sync_all)
    }
    fn seal_with(&mut self, sync: impl FnOnce(&File) -> io::Result<()>) -> io::Result<()> {
        self.require_private()?;
        if self.sealed {
            return Err(invalid("spool is already sealed"));
        }
        sync(self.stage.file_mut())?;
        self.stage.file_mut().seek(SeekFrom::Start(0))?;
        self.sealed = true;
        Ok(())
    }
    /// Seal a disposable, authenticated cache entry without requesting crash
    /// durability. Only cache owners that can reconstruct missing/corrupt bytes
    /// may use this boundary. Evidence, manifests and ordinary output must use
    /// `seal`, whose sync behavior is unchanged.
    pub fn seal_rebuildable_cache(&mut self) -> io::Result<()> {
        self.require_private()?;
        if self.sealed {
            return Err(invalid("spool is already sealed"));
        }
        self.stage.file_mut().seek(SeekFrom::Start(0))?;
        self.sealed = true;
        Ok(())
    }
    /// No-replace, same-directory publication only, using the unchanged native
    /// NULL-root rename. NotPublished means an existing entry won the race and
    /// requires caller verification. Other failed syscalls remain Unknown.
    pub fn publish_name<C: FileControl>(
        &mut self,
        name: &std::ffi::OsStr,
        control: &mut C,
    ) -> Result<StagedPublication, C::Error> {
        self.publish_name_with(name, control, |stage, path| stage.rename(path, false))
    }
    /// Replace one existing same-directory entry using the established POSIX
    /// rename. Open project readers keep the old file; later readers see the
    /// complete replacement. This does not allow any cross-directory move.
    pub fn replace_name<C: FileControl>(
        &mut self,
        name: &std::ffi::OsStr,
        control: &mut C,
    ) -> Result<StagedPublication, C::Error> {
        self.publish_name_with(name, control, |stage, path| stage.rename(path, true))
    }
    /// Cache-only nonblocking attempt. Keep Stage's deny-write/delete sharing:
    /// the original private file cannot move to another parent before the
    /// NULL-root same-directory rename. Only an exact synchronous native
    /// STATUS_SHARING_VIOLATION grants SharingBlocked; unknown completion and
    /// all other errors remain terminal. This method neither sleeps nor resets
    /// a deadline. Ordinary stages cannot opt in, even after cache-style seal.
    pub fn try_replace_rebuildable_cache_name<C: FileControl>(
        &mut self,
        name: &std::ffi::OsStr,
        control: &mut C,
    ) -> Result<CacheReplaceAttempt, C::Error> {
        if !self.rebuildable_cache {
            return Err(invalid("cache replacement requires a rebuildable-cache stage").into());
        }
        self.publish_attempt_with(name, control, |stage, path| {
            stage.rename_attempt(path, true)
        })
    }
    fn publish_name_with<C: FileControl>(
        &mut self,
        name: &std::ffi::OsStr,
        control: &mut C,
        rename: impl FnOnce(&mut Stage, &Path) -> io::Result<()>,
    ) -> Result<StagedPublication, C::Error> {
        self.publish_attempt_with(name, control, |stage, path| {
            rename(stage, path).map(|()| RenameAttempt::Published)
        })
        .map(|attempt| match attempt {
            CacheReplaceAttempt::Complete(state) => state,
            CacheReplaceAttempt::SharingBlocked => {
                unreachable!("ordinary rename returns io errors")
            }
        })
    }
    fn publish_attempt_with<C: FileControl>(
        &mut self,
        name: &std::ffi::OsStr,
        control: &mut C,
        rename: impl FnOnce(&mut Stage, &Path) -> io::Result<RenameAttempt>,
    ) -> Result<CacheReplaceAttempt, C::Error> {
        self.require_private().map_err(C::Error::from)?;
        if !self.sealed
            || name.len() > 32768
            || !valid_name(name)
            || name.encode_wide().any(|unit| [47, 92].contains(&unit))
        {
            return Err(invalid("publication requires a sealed spool and one simple name").into());
        }
        self.parent.verify_controlled(control)?;
        control.charge(FileCost {
            io_operations: 2,
            allocation_bytes: name.len() as u64 * 16 + 1024,
            work_units: name.len() as u64 * 16 + 1024,
        })?;
        control.check()?;
        self.publication = StagedPublication::Unknown;
        self.stage.published = true; // Drop must not delete a possibly committed object.
        match rename(&mut self.stage, Path::new(name)) {
            Ok(RenameAttempt::Published) => {
                self.publication = StagedPublication::Published;
                Ok(CacheReplaceAttempt::Complete(self.publication))
            }
            Ok(RenameAttempt::SharingBlocked) => {
                self.publication = StagedPublication::NotPublished;
                self.stage.published = false;
                Ok(CacheReplaceAttempt::SharingBlocked)
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                self.publication = StagedPublication::NotPublished;
                self.stage.published = false;
                Ok(CacheReplaceAttempt::Complete(self.publication))
            }
            Err(error) => Err(error.into()),
        }
    }
    /// Consume once, by original handle. A failed discard is reported and not
    /// retried in Drop. Published/Unknown objects are never discarded here.
    pub fn close(self) -> Vec<io::Error> {
        self.close_with(super::super::close_file)
    }
    fn close_with(mut self, close: impl FnOnce(File) -> io::Result<()>) -> Vec<io::Error> {
        let mut errors = Vec::new();
        if self.publication == StagedPublication::NotPublished {
            if let Err(error) = self.stage.discard() {
                errors.push(error);
            }
        }
        self.stage.published = true;
        if let Some(file) = self.stage.file.take() {
            if let Err(error) = close(file) {
                errors.push(error);
            }
        }
        errors
    }
}
impl Read for StagedFile<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.require_private()?;
        if !self.sealed {
            return Err(invalid("unsealed spool cannot be read"));
        }
        let admitted = bytes.len().min(65536);
        self.stage.file_mut().read(&mut bytes[..admitted])
    }
}
mod private_spool;
pub use private_spool::PrivateSpool;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod zip_tests;

#[cfg(test)]
mod rebuildable_cache_tests {
    use super::*;
    #[test]
    fn rebuildable_seal_keeps_original_owner_and_complete_atomic_visibility() {
        let path = std::env::temp_dir().join(format!(
            "lychee-cache-seal-{}",
            crate::random::token().unwrap()
        ));
        std::fs::create_dir(&path).unwrap();
        let directory = Directory::open(&path).unwrap();
        let mut stage = StagedFile::create(&directory, &mut Uncontrolled).unwrap();
        let temporary = stage.name().to_owned();
        stage.write_all(b"complete cache").unwrap();
        stage.seal_rebuildable_cache().unwrap();
        assert!(stage.write_all(b"late mutation").is_err());
        assert!(stage.seal().is_err());
        let mut bytes = Vec::new();
        stage.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"complete cache");
        assert_eq!(
            stage
                .publish_name(std::ffi::OsStr::new("entry"), &mut Uncontrolled)
                .unwrap(),
            StagedPublication::Published
        );
        // A reported checked-close failure retains the complete publication;
        // no path cleanup or second close can erase it or disguise success.
        let errors = stage.close_with(|file| {
            crate::filesystem::close_file(file)?;
            Err(io::Error::other("injected close failure"))
        });
        assert_eq!(errors.len(), 1);
        assert_eq!(
            std::fs::read(path.join("entry")).unwrap(),
            b"complete cache"
        );
        assert!(!path.join(temporary).exists());
        directory.close().unwrap();
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn rebuildable_seal_no_replace_preserves_complete_concurrent_winner() {
        let path = std::env::temp_dir().join(format!(
            "lychee-cache-winner-{}",
            crate::random::token().unwrap()
        ));
        std::fs::create_dir(&path).unwrap();
        let directory = Directory::open(&path).unwrap();
        let mut stage = StagedFile::create(&directory, &mut Uncontrolled).unwrap();
        stage.write_all(b"candidate").unwrap();
        stage.seal_rebuildable_cache().unwrap();
        std::fs::write(path.join("entry"), b"winner").unwrap();
        assert_eq!(
            stage
                .publish_name(std::ffi::OsStr::new("entry"), &mut Uncontrolled)
                .unwrap(),
            StagedPublication::NotPublished
        );
        assert!(stage.close().is_empty());
        assert_eq!(std::fs::read(path.join("entry")).unwrap(), b"winner");
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 1);
        directory.close().unwrap();
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[cfg(test)]
mod cache_slot_tests;
