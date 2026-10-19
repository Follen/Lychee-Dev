//! Immutable reads through the original disk/query owner. A separate read-only
//! capability admits zero growth even when existing bytes exceed a reduced
//! policy; it never exposes the deferred-admission owner to a writer.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    io::{self, Read, Seek, SeekFrom},
};

pub const MAX_GENERATION_BYTES: usize = 1 << 20;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedReadReport {
    pub configured_max_bytes: u64,
    pub existing: DerivedUsage,
    pub over_budget: bool,
    pub coverage: Coverage,
}

/// Holds the existing quota lease for the complete read lifetime. The private
/// reservation is never returned or exposed through Deref. No payload bytes are
/// written. Acquisition retains the established empty-directory/lock setup.
pub struct DerivedReadLease<'a> {
    owner: DerivedReservation<'a>,
}
impl<'a> DerivedReadLease<'a> {
    pub(in crate::workspace) fn acquire(
        resources: &'a DataResources,
        home: &Path,
        limit: u64,
        control: &'a OperationControl,
        supplemental: &'a dyn Fn() -> Result<()>,
    ) -> Result<Self> {
        DerivedReservation::acquire_inner(
            Some(resources),
            home,
            limit,
            AdditionalPeak::default(),
            control,
            Some(supplemental),
            true,
        )
        .map(|owner| Self { owner })
    }
    pub fn report(&self) -> DerivedReadReport {
        let report = self.owner.report();
        DerivedReadReport {
            configured_max_bytes: report.configured_max_bytes,
            existing: report.existing,
            over_budget: report.admitted_peak_bytes > report.configured_max_bytes,
            coverage: report.coverage.clone(),
        }
    }
    pub fn read_generation(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.owner.read_generation(key)
    }
    pub fn open_object(&self, sha256: &str, bytes: u64) -> Result<DerivedObjectReader<'_>> {
        self.owner.open_object(sha256, bytes)
    }
    pub fn finish<T>(self, result: Result<T>) -> Result<T> {
        self.owner.finish(result)
    }
}

fn integrity(message: &str) -> ToolkitError {
    ToolkitError::new("vault.data_cache_integrity", message, 4)
}

impl DerivedReservation<'_> {
    /// Only a valid absent generation is a miss. Read from one deny-write/delete
    /// handle, with a 1 MiB allocation bound and EOF verification. The key is a
    /// logical generation identity, not this JSON file's content digest: the
    /// Data consumer must authenticate all manifest semantics before reuse.
    pub fn read_generation(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.read_generation_with_close(key, storage::close_file)
    }
    fn read_generation_with_close(
        &self,
        key: &str,
        close: impl FnOnce(fs::File) -> io::Result<()>,
    ) -> Result<Option<Vec<u8>>> {
        self.control.check()?;
        if !valid_digest(key, 64) || key.bytes().any(|byte| byte.is_ascii_uppercase()) {
            return Err(ToolkitError::invalid(
                "Data generation requires lowercase SHA-256 key",
            ));
        }
        let directory = self.read_directory(&self.paths.generations);
        let Some(mut file) = directory.open_optional_reader_owner_controlled(
            OsStr::new(&format!("{key}.json")),
            &mut Controlled(&self.control),
        )?
        else {
            self.control.check()?;
            return Ok(None);
        };
        let result = (|| {
            self.control.check()?;
            let metadata = file.metadata()?;
            if !metadata.is_file() || metadata.len() > MAX_GENERATION_BYTES as u64 {
                return Err(integrity("Data generation kind or bounded length differs"));
            }
            let length = metadata.len() as usize;
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(length).map_err(|_| {
                ToolkitError::invalid("Cannot allocate bounded Data generation manifest")
            })?;
            bytes.resize(length, 0);
            for block in bytes.chunks_mut(65536) {
                self.control.check()?;
                file.read_exact(block)?;
                self.control.check()?;
            }
            self.control.check()?;
            if file.read(&mut [0])? != 0 {
                return Err(integrity("Data generation exceeds its measured length"));
            }
            self.control.check()?;
            Ok(Some(bytes))
        })();
        join(
            result,
            close(file).err().map(ToolkitError::io),
            &self.control,
        )
    }

    /// Authenticate exact size, full SHA-256 and EOF before returning the SAME
    /// immutable handle, rewound for its reader. A missing referenced object is
    /// corruption, never a cache miss. No quota or query owner is reacquired.
    pub fn open_object(&self, sha256: &str, bytes: u64) -> Result<DerivedObjectReader<'_>> {
        self.control.check()?;
        let path = self.paths.object(sha256)?;
        let root = self.read_directory(&self.paths.objects);
        let Some(anchor) = root.open_optional_reader_owner_controlled(
            OsStr::new(&sha256[..2]),
            &mut Controlled(&self.control),
        )?
        else {
            self.control.check()?;
            return Err(integrity("Referenced Data object shard is missing"));
        };
        let opened = (|| {
            self.control.check()?;
            if !anchor.metadata()?.is_dir() {
                return Err(integrity("Referenced Data object shard is not a directory"));
            }
            Directory::open_controlled(
                path.parent().expect("object shard"),
                &mut Controlled(&self.control),
            )
        })();
        let closed = finish_file(anchor, Ok(()), &self.control);
        let directory = match opened {
            Ok(directory) => match closed {
                Ok(()) => directory,
                Err(error) => return join(Err(error), close_directory(directory), &self.control),
            },
            Err(error) => return join(Err(error), closed.err(), &self.control),
        };
        let opened = (|| {
            directory
                .open_optional_reader_owner_controlled(
                    OsStr::new(&sha256[2..]),
                    &mut Controlled(&self.control),
                )?
                .ok_or_else(|| integrity("Referenced Data object is missing"))
        })();
        let file = match opened {
            Ok(file) => file,
            Err(error) => return join(Err(error), close_directory(directory), &self.control),
        };
        let mut reader = DerivedObjectReader {
            owner: self,
            file: Some(file),
            directory: Some(directory),
            bytes,
            failure: None,
        };
        match reader.authenticate(sha256) {
            Ok(()) => Ok(reader),
            Err(error) => reader.close_after(Err(error)),
        }
    }

    fn read_directory(&self, path: &Path) -> &Directory {
        self.directories
            .iter()
            .find(|directory| directory.path() == path)
            .expect("disk owner retains its fixed read directory")
    }
}

/// The borrow prevents releasing quota/query admission before the original
/// reader closes. Read and Seek preserve typed failures for codec adapters via
/// error(); the raw File and Directory are never exposed.
pub struct DerivedObjectReader<'a> {
    owner: &'a DerivedReservation<'a>,
    file: Option<fs::File>,
    directory: Option<Directory>,
    bytes: u64,
    failure: Option<ToolkitError>,
}
impl DerivedObjectReader<'_> {
    pub fn len(&self) -> u64 {
        self.bytes
    }
    pub fn is_empty(&self) -> bool {
        self.bytes == 0
    }
    pub fn error(&self) -> Option<&ToolkitError> {
        self.failure.as_ref()
    }
    fn remember<T>(&mut self, result: Result<T>) -> Result<T> {
        if let Err(error) = &result {
            self.failure.get_or_insert_with(|| error.clone());
        }
        result
    }
    pub fn read_checked(&mut self, out: &mut [u8]) -> Result<usize> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let result = (|| {
            self.owner.control.check()?;
            let length = out.len().min(65536);
            let count = self
                .file
                .as_mut()
                .expect("owned reader")
                .read(&mut out[..length])?;
            self.owner.control.check()?;
            Ok(count)
        })();
        self.remember(result)
    }
    pub fn read_exact_checked(&mut self, mut out: &mut [u8]) -> Result<()> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        while !out.is_empty() {
            let count = self.read_checked(out)?;
            if count == 0 {
                return self.remember(Err(ToolkitError::io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Data object ended before the requested read completed",
                ))));
            }
            out = &mut out[count..];
        }
        let result = self.owner.control.check();
        self.remember(result)
    }
    pub fn seek_checked(&mut self, from: SeekFrom) -> Result<u64> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let result = (|| {
            self.owner.control.check()?;
            let position = self.file.as_mut().expect("owned reader").seek(from)?;
            self.owner.control.check()?;
            Ok(position)
        })();
        self.remember(result)
    }
    fn authenticate(&mut self, sha256: &str) -> Result<()> {
        self.owner.control.check()?;
        let metadata = self.file.as_ref().expect("owned reader").metadata()?;
        if !metadata.is_file() || metadata.len() != self.bytes {
            return Err(integrity("Referenced Data object kind or length differs"));
        }
        let mut buffer = [0; 65536];
        let mut remaining = self.bytes;
        let mut digest = Sha256::new();
        while remaining != 0 {
            let count = remaining.min(buffer.len() as u64) as usize;
            self.read_exact_checked(&mut buffer[..count])?;
            digest.update(&buffer[..count]);
            remaining -= count as u64;
            #[cfg(test)]
            HASHED_BYTES.with(|progress| progress.set(self.bytes - remaining));
        }
        if self.read_checked(&mut [0])? != 0 || hex::encode(digest.finalize()) != sha256 {
            return Err(integrity("Referenced Data object content differs"));
        }
        self.seek_checked(SeekFrom::Start(0))?;
        Ok(())
    }
    pub fn close(self) -> Result<()> {
        self.close_after(Ok(()))
    }
    /// Pass the consumer's original typed failure, using error() first if its
    /// codec erased an I/O cause. Always consumes/checked-closes every owner.
    pub fn close_after<T>(self, result: Result<T>) -> Result<T> {
        self.close_after_with(result, storage::close_file, close_directory)
    }
    fn close_after_with<T>(
        mut self,
        result: Result<T>,
        close_file: impl FnOnce(fs::File) -> io::Result<()>,
        close_dir: impl FnOnce(Directory) -> Vec<ToolkitError>,
    ) -> Result<T> {
        let result = match (result, self.failure.as_ref()) {
            (Ok(_), Some(error)) => Err(error.clone()),
            (result, _) => result,
        };
        let mut cleanup = Vec::new();
        if let Some(file) = self.file.take() {
            cleanup.extend(close_file(file).err().map(ToolkitError::io));
        }
        if let Some(directory) = self.directory.take() {
            cleanup.extend(close_dir(directory));
        }
        join(result, cleanup, &self.owner.control)
    }
}
impl Read for DerivedObjectReader<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.read_checked(out).map_err(io::Error::other)
    }
}
impl Seek for DerivedObjectReader<'_> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.seek_checked(from).map_err(io::Error::other)
    }
}
impl Drop for DerivedObjectReader<'_> {
    fn drop(&mut self) {
        if let Some(file) = self.file.take() {
            let _ = storage::close_file(file);
        }
        if let Some(directory) = self.directory.take() {
            let _ = directory.close();
        }
    }
}

#[cfg(test)]
thread_local! { static HASHED_BYTES: Cell<u64> = const { Cell::new(0) }; }
#[cfg(test)]
mod tests;
