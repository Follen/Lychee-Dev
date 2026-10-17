//! A same-command intermediate, never a publication or recovery owner.
use super::*;

/// An original-handle spool that cannot publish or expose its writable File.
/// Its bytes are consumed and independently checked before a different, durable
/// stage may publish. It has no crash-recovery promise. Checked close always
/// attempts discard and reports any discard/close failure rather than promising
/// deletion. This type deliberately has no Deref/into-inner escape.
///
/// ```compile_fail
/// use lycheedev_windows::filesystem::destination::{FileControl, PrivateSpool};
/// fn cannot_publish<C: FileControl>(mut spool: PrivateSpool<'_>, control: &mut C) {
///     spool.publish_name(std::ffi::OsStr::new("blob"), control);
/// }
/// ```
pub struct PrivateSpool<'a> {
    stage: StagedFile<'a>,
}
impl<'a> PrivateSpool<'a> {
    pub fn create<C: FileControl>(parent: &'a Directory, control: &mut C) -> Result<Self, C::Error> {
        StagedFile::create(parent, control).map(|stage| Self { stage })
    }
    pub fn name(&self) -> &str {
        self.stage.name()
    }
    pub fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.stage.write_all(bytes)
    }
    /// Finish writes and rewind the original synchronous, unbuffered File.
    /// This is read visibility, not sync_all/crash durability. Later writes and
    /// repeated transitions fail; neither this owner nor its reader can publish.
    /// A failed seek leaves this same owner available for checked discard/close.
    pub fn finish_writes(&mut self) -> io::Result<()> {
        self.finish_writes_with(|file| file.seek(SeekFrom::Start(0)).map(|_| ()))
    }
    fn finish_writes_with(&mut self, rewind: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
        self.stage.require_private()?;
        if self.stage.sealed {
            return Err(invalid("private spool is already read-ready"));
        }
        rewind(self.stage.stage.file_mut())?;
        self.stage.sealed = true;
        Ok(())
    }
    /// All real discard/close errors survive; no destructor retries a failed
    /// checked discard, and no pathname is used to reclaim somebody else's file.
    pub fn close(self) -> Vec<io::Error> {
        self.stage.close()
    }
}
impl Read for PrivateSpool<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.stage.read(bytes)
    }
}

#[cfg(test)]
mod tests;
