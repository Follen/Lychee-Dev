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
    exact: Option<ExactLength>,
}
struct ExactLength {
    expected: u64,
    written: u64,
    poisoned: bool,
}
impl<'a> PrivateSpool<'a> {
    pub fn create<C: FileControl>(
        parent: &'a Directory,
        control: &mut C,
    ) -> Result<Self, C::Error> {
        StagedFile::create(parent, control).map(|stage| Self { stage, exact: None })
    }
    pub fn name(&self) -> &str {
        self.stage.name()
    }
    /// Reserve a caller-admitted exact logical length on this original owner.
    /// This does not promise physical allocation, sparse-file behavior, or an
    /// interruptible syscall. Callers check their original control before/after.
    /// The file is not read-ready: only actual successful writes count toward
    /// completion, and no pre-sized zero suffix can become authenticated input.
    pub fn reserve_exact_len(&mut self, expected: u64) -> io::Result<()> {
        self.reserve_exact_len_with(expected, |file, bytes| file.set_len(bytes), File::sync_all)
    }
    fn reserve_exact_len_with(
        &mut self,
        expected: u64,
        set_len: impl FnOnce(&File, u64) -> io::Result<()>,
        sync: impl FnOnce(&File) -> io::Result<()>,
    ) -> io::Result<()> {
        self.stage.require_private()?;
        if self.stage.sealed
            || self.exact.is_some()
            || self.stage.stage.file_mut().stream_position()? != 0
            || self.stage.stage.file_mut().metadata()?.len() != 0
        {
            return Err(invalid(
                "exact-length reservation requires a fresh private spool",
            ));
        }
        // Install the poisoned state before any fallible native operation.
        // A partial set_len/seek/sync failure cannot resume ordinary writes.
        self.exact = Some(ExactLength {
            expected,
            written: 0,
            poisoned: true,
        });
        let file = self.stage.stage.file_mut();
        set_len(file, expected)?;
        file.seek(SeekFrom::Start(0))?;
        sync(file)?;
        self.exact.as_mut().expect("reserved length").poisoned = false;
        Ok(())
    }
    pub fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        let Some(exact) = self.exact.as_mut() else {
            return self.stage.write_all(bytes);
        };
        if exact.poisoned {
            return Err(invalid(
                "exact-length spool is stopped after an I/O failure",
            ));
        }
        let next = exact
            .written
            .checked_add(bytes.len() as u64)
            .filter(|&length| length <= exact.expected)
            .ok_or_else(|| invalid("write exceeds exact-length spool reservation"))?;
        exact.poisoned = true;
        self.stage.write_all(bytes)?;
        exact.written = next;
        exact.poisoned = false;
        Ok(())
    }
    /// Finish writes and rewind the original synchronous, unbuffered File.
    /// This is read visibility, not sync_all/crash durability. Later writes and
    /// repeated transitions fail; neither this owner nor its reader can publish.
    /// A failed seek leaves this same owner available for checked discard/close.
    pub fn finish_writes(&mut self) -> io::Result<()> {
        self.finish_writes_with(|file| file.seek(SeekFrom::Start(0)).map(|_| ()))
    }
    fn finish_writes_with(
        &mut self,
        rewind: impl FnOnce(&mut File) -> io::Result<()>,
    ) -> io::Result<()> {
        self.stage.require_private()?;
        if self.stage.sealed {
            return Err(invalid("private spool is already read-ready"));
        }
        if let Some(exact) = &self.exact {
            if exact.poisoned
                || exact.written != exact.expected
                || self.stage.stage.file_mut().metadata()?.len() != exact.expected
            {
                return Err(invalid(
                    "exact-length spool has not received its complete body",
                ));
            }
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

#[cfg(test)]
mod exact_length_tests;
