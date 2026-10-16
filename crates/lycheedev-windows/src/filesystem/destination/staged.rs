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

/// Borrows its original strict Directory and never exposes a writable File.
/// A sealed spool may be reread through the same handle but never written again.
pub struct StagedFile<'a> {
    stage: Stage,
    parent: &'a Directory,
    publication: StagedPublication,
    sealed: bool,
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
        })
    }
    pub fn name(&self) -> &str {
        &self.stage.name
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
        self.require_private()?;
        if self.sealed {
            return Err(invalid("spool is already sealed"));
        }
        self.stage.file_mut().sync_all()?;
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
    fn publish_name_with<C: FileControl>(
        &mut self,
        name: &std::ffi::OsStr,
        control: &mut C,
        rename: impl FnOnce(&mut Stage, &Path) -> io::Result<()>,
    ) -> Result<StagedPublication, C::Error> {
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
            Ok(()) => {
                self.publication = StagedPublication::Published;
                Ok(self.publication)
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                self.publication = StagedPublication::NotPublished;
                self.stage.published = false;
                Ok(self.publication)
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
#[cfg(test)]
mod tests;

#[cfg(test)]
mod zip_tests;
