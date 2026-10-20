//! Identity-bound cleanup of files created or authenticated by the caller.
//! No function accepts a pathname for deletion. Every deletion disposition is
//! applied to the original owned handle, which is returned for checked close.
use std::{
    fs::{File, OpenOptions},
    io,
    mem::size_of,
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::Path,
};
use windows::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{FILE_DISPOSITION_INFO, FileDispositionInfo, SetFileInformationByHandle},
};
const READ_DELETE: u32 = 0x100000 | 0x10000 | 1 | 0x80; // SYNCHRONIZE | DELETE | READ_DATA/LIST | READ_ATTRIBUTES
const REPARSE_BACKUP: u32 = 0x00200000 | 0x02000000; // OPEN_REPARSE_POINT | BACKUP_SEMANTICS
fn mark_original(file: &File) -> io::Result<()> {
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: file remains the same live, uniquely owned File borrowed for the
    // duration of this synchronous call. The fixed-size stack record is fully
    // initialized and lives until return; Windows retains neither pointer.
    // All constructors request DELETE access. This changes only the object
    // identified by the handle, never a reopened or substituted directory entry.
    unsafe {
        SetFileInformationByHandle(
            HANDLE(file.as_raw_handle()),
            FileDispositionInfo,
            (&info as *const FILE_DISPOSITION_INFO).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    }
    .map_err(|e| io::Error::from_raw_os_error((e.code().0 as u32 & 0xffff) as i32))
}
/// A CREATE_NEW token owns only its original temporary object. A cooperating
/// caller may discard it even when writing failed before content validation.
pub struct OwnedTemporary {
    file: File,
}
impl OwnedTemporary {
    pub fn create(path: &Path) -> io::Result<Self> {
        // Share modes preserve the historical atomic writer. A substituted
        // pathname cannot redirect discard: mark_original uses this File.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .access_mode(READ_DELETE | 2 | 0x100)
            .share_mode(1 | 2 | 4)
            .open(path)?;
        Ok(Self { file })
    }
    pub fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }
    pub fn into_file(self) -> File {
        self.file
    }
    pub fn discard(self) -> (File, io::Result<()>) {
        let result = mark_original(&self.file);
        (self.file, result)
    }
}
/// A candidate denies write/delete/reparse replacement while it is validated.
/// An unsuccessful validation returns the original candidate to checked-close;
/// it creates no deletion authority and performs no mutation.
pub struct CleanupCandidate {
    file: File,
    directory: bool,
}
pub struct AuthenticatedCleanup {
    file: File,
}
impl CleanupCandidate {
    // Only the sibling relative DELETE opener may transfer this capability.
    pub(super) fn from_relative_file(file: File) -> Self {
        Self {
            file,
            directory: false,
        }
    }
    pub fn open(path: &Path, directory: bool) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .access_mode(READ_DELETE)
            .share_mode(1)
            .custom_flags(REPARSE_BACKUP)
            .open(path)?;
        Ok(Self { file, directory })
    }
    pub fn authenticate<E: From<io::Error>>(
        mut self,
        validate: impl FnOnce(&mut File) -> Result<(), E>,
    ) -> Result<AuthenticatedCleanup, (E, Self)> {
        let result = (|| {
            let info = self.file.metadata().map_err(E::from)?;
            if info.file_attributes() & 0x400 != 0
                || if self.directory {
                    !info.is_dir()
                } else {
                    !info.is_file()
                }
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "cleanup object is redirected or has another kind",
                )
                .into());
            }
            validate(&mut self.file)
        })();
        match result {
            Ok(()) => Ok(AuthenticatedCleanup { file: self.file }),
            Err(e) => Err((e, self)),
        }
    }
    pub fn into_file(self) -> File {
        self.file
    }
}
impl AuthenticatedCleanup {
    /// Consume deletion authority exactly once, even if the disposition fails.
    /// The caller must close the returned File and retain both error results.
    pub fn discard(self) -> (File, io::Result<()>) {
        let result = mark_original(&self.file);
        (self.file, result)
    }
    pub fn into_file(self) -> File {
        self.file
    }
}
/// Hold a directory ancestor without granting DELETE access. The caller checks
/// its directory/reparse attributes before opening the next path component and
/// keeps every ancestor alive through all dependent child operations.
pub fn open_ancestor(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .share_mode(1)
        .custom_flags(REPARSE_BACKUP)
        .open(path)
}
#[cfg(test)]
#[path = "owned_cleanup_tests.rs"]
mod tests;
