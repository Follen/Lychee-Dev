//! Immutable reads through the original disk/query owner. A separate read-only
//! capability admits zero growth even when existing bytes exceed a reduced
//! policy; it never exposes the deferred-admission owner to a writer.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    io::{self, Read, Seek, SeekFrom},
    sync::Arc,
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

/// Holds quota for existing borrowed reads. The private reservation is never
/// exposed through Deref and no payload bytes are written. Acquisition keeps the
/// established directory setup. OwnedDerivedReadLease is the cursor capability.
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

/// The same independent query budget across short admission and owned reads.
/// Cloning this Arc preserves its identity; no fresh budget is manufactured.
pub type DerivedReadCheck = Arc<dyn Fn() -> Result<()> + Send + Sync>;

/// A short quota lease that retains the exact owned check supplied at admission.
/// Its borrow ends at object handoff; the containing query must checked-close
/// all returned readers before closing its original DataResources admission.
pub struct OwnedDerivedReadLease<'a> {
    lease: DerivedReadLease<'a>,
    supplemental: DerivedReadCheck,
}
impl<'a> OwnedDerivedReadLease<'a> {
    pub(in crate::workspace) fn acquire(
        resources: &'a DataResources,
        home: &Path,
        limit: u64,
        control: &'a OperationControl,
        supplemental: &'a DerivedReadCheck,
    ) -> Result<Self> {
        DerivedReadLease::acquire(resources, home, limit, control, supplemental.as_ref()).map(
            |lease| Self {
                lease,
                supplemental: Arc::clone(supplemental),
            },
        )
    }
    pub fn report(&self) -> DerivedReadReport {
        self.lease.report()
    }
    pub fn read_generation(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.lease.read_generation(key)
    }
    /// Charge caller-owned raw/parsing memory only after this exact generation
    /// handle proves a bounded length. Absence never invokes admission. Keep
    /// the callback's retained guard alive until the returned bytes are dropped.
    pub fn read_generation_with_admission(
        &self,
        key: &str,
        admit: impl FnOnce(usize) -> Result<()>,
    ) -> Result<Option<Vec<u8>>> {
        self.lease.owner.read_generation_with_admission(key, admit)
    }
    /// Authenticate manifest semantics before calling. This consumes quota and
    /// returns the original immutable handle, fully SHA-authenticated and rewound.
    /// Only an absent shard/body from its original controlled open is a miss;
    /// redirected, changed, corrupt, stopped or failed-close reads remain errors.
    ///
    /// Windows Reader permits other readers but denies write/delete; its shard
    /// Directory retains all ancestors. A cleanup candidate needing DELETE must
    /// try once under quota and skip only its exact sharing violation. It must
    /// separately establish all generation/evidence reference deletion rights.
    pub fn open_pinned_object(
        self,
        sha256: &str,
        bytes: u64,
    ) -> Result<Option<OwnedDerivedObjectReader>> {
        self.open_pinned_object_with(
            sha256,
            bytes,
            |owner| owner.finish(Ok(())),
            storage::close_file,
            close_directory,
        )
    }
    fn open_pinned_object_with(
        self,
        sha256: &str,
        bytes: u64,
        finish_owner: impl FnOnce(DerivedReservation<'a>) -> Result<()>,
        close_file: impl FnOnce(fs::File) -> io::Result<()>,
        close_dir: impl FnOnce(Directory) -> Vec<ToolkitError>,
    ) -> Result<Option<OwnedDerivedObjectReader>> {
        let opened = match self.lease.owner.open_object_handle(sha256, bytes, true) {
            Ok(reader) => reader,
            Err(error) => return self.lease.owner.finish(Err(error)),
        };
        let Some(reader) = opened else {
            return finish_owner(self.lease.owner).map(|()| None);
        };
        let mut reader =
            reader.into_owned(self.lease.owner.control.control.clone(), self.supplemental);
        // Keep the original reader separate from finish's successful value:
        // failed quota verification/close must explicitly report its cleanup.
        // Mark a native length-validated acquisition under the short quota
        // owner. Recency is advisory: a later hash failure may leave a recent
        // marker, but can never authorize eviction or hide corruption. Preserve
        // checked quota-close before full hashing and the original pin lifetime.
        let marked = self.lease.owner.touch_object(sha256);
        let result = crate::metadata::historical_write::finish_historical(
            marked, finish_owner(self.lease.owner))
            .and_then(|()| reader.authenticate(sha256));
        match result {
            Ok(()) => Ok(Some(reader)),
            Err(error) => reader.close_after_with(Err(error), close_file, close_dir),
        }
    }
    pub fn finish<T>(self, result: Result<T>) -> Result<T> {
        self.lease.finish(result)
    }
}

// Both capabilities share the same exact I/O, authentication and close code.
// An owned reader does not itself guarantee query admission: its concrete Data
// owner must retain that admission until every reader has checked-closed.
pub type DerivedObjectReader<'a> = ObjectReader<BorrowedReaderChecks<'a>>;
pub type OwnedDerivedObjectReader = ObjectReader<OwnedReaderChecks>;

/// Type-level separation preserves borrowed callback compatibility without
/// making the independently owned production reader inherit a non-Send borrow.
pub struct BorrowedReaderChecks<'a> {
    _resources: Option<&'a DataResources>,
    checks: Checks<'a>,
}
pub struct OwnedReaderChecks {
    control: OperationControl,
    supplemental: DerivedReadCheck,
}
pub trait ReaderCheck {
    fn checks(&self) -> Checks<'_>;
}
impl ReaderCheck for BorrowedReaderChecks<'_> {
    fn checks(&self) -> Checks<'_> { self.checks }
}
impl ReaderCheck for OwnedReaderChecks {
    fn checks(&self) -> Checks<'_> {
        Checks { control: &self.control, supplemental: Some(self.supplemental.as_ref()) }
    }
}

fn integrity(message: &str) -> ToolkitError {
    ToolkitError::new("vault.data_cache_integrity", message, 4)
}

impl<'a> DerivedReservation<'a> {
    /// Only a valid absent generation is a miss. Read from one deny-write/delete
    /// handle, with a 1 MiB allocation bound and EOF verification. The key is a
    /// logical generation identity, not this JSON file's content digest: the
    /// Data consumer must authenticate all manifest semantics before reuse.
    pub fn read_generation(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.read_generation_with_close(key, storage::close_file)
    }
    /// The same handle supplies both the validated length and the returned
    /// bytes. The caller retains any reservation created by admit through all
    /// use of those bytes. This does not perform a separate existence probe.
    pub fn read_generation_with_admission(
        &self,
        key: &str,
        admit: impl FnOnce(usize) -> Result<()>,
    ) -> Result<Option<Vec<u8>>> {
        self.read_generation_admitted_with_close(key, admit, storage::close_file)
    }
    fn read_generation_with_close(
        &self,
        key: &str,
        close: impl FnOnce(fs::File) -> io::Result<()>,
    ) -> Result<Option<Vec<u8>>> {
        self.read_generation_admitted_with_close(key, |_| Ok(()), close)
    }
    fn read_generation_admitted_with_close(
        &self,
        key: &str,
        admit: impl FnOnce(usize) -> Result<()>,
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
            admit(length)?;
            // Admission can cancel the original operation while succeeding.
            // Observe that stop before reserving or initializing the buffer.
            self.control.check()?;
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
        let mut reader = self
            .open_object_handle(sha256, bytes, false)?
            .expect("required object open never returns absence");
        match reader.authenticate(sha256) {
            Ok(()) => match self.touch_object(sha256) {
                Ok(()) => Ok(reader),
                Err(error) => reader.close_after(Err(error)),
            },
            Err(error) => reader.close_after(Err(error)),
        }
    }

    // Public borrowed reads deliberately retain the quota borrow. The owned
    // capability alone transfers these handles and their unchanged controls.
    fn open_object_handle(
        &self,
        sha256: &str,
        bytes: u64,
        optional: bool,
    ) -> Result<Option<DerivedObjectReader<'a>>> {
        self.control.check()?;
        let path = self.paths.object(sha256)?;
        let root = self.read_directory(&self.paths.objects);
        let Some(anchor) = root.open_optional_reader_owner_controlled(
            OsStr::new(&sha256[..2]),
            &mut Controlled(&self.control),
        )?
        else {
            self.control.check()?;
            return if optional {
                Ok(None)
            } else {
                Err(integrity("Referenced Data object shard is missing"))
            };
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
        let opened = directory.open_optional_reader_owner_controlled(
            OsStr::new(&sha256[2..]),
            &mut Controlled(&self.control),
        );
        let file = match opened {
            Ok(Some(file)) => file,
            Ok(None) => {
                let absent = if optional {
                    Ok(None)
                } else {
                    Err(integrity("Referenced Data object is missing"))
                };
                return join(absent, close_directory(directory), &self.control);
            }
            Err(error) => return join(Err(error), close_directory(directory), &self.control),
        };
        let reader = ObjectReader {
            control: BorrowedReaderChecks { _resources: self._resources, checks: self.control },
            file: Some(file),
            directory: Some(directory),
            bytes,
            failure: None,
        };
        // Transfer the original reader before any fallible post-open check.
        match reader.validate_kind_length() {
            Ok(()) => Ok(Some(reader)),
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

/// Owns one immutable physical read pin and its complete ancestry. Borrowed
/// reads retain the original admission; owned cursor reads retain its exact
/// controls and rely on the containing query to close them before admission.
/// Read/Seek preserve typed failures through error(). Raw owners are private.
pub struct ObjectReader<C: ReaderCheck> {
    control: C,
    file: Option<fs::File>,
    directory: Option<Directory>,
    bytes: u64,
    failure: Option<ToolkitError>,
}
impl ObjectReader<BorrowedReaderChecks<'_>> {
    fn into_owned(
        mut self,
        control: OperationControl,
        supplemental: DerivedReadCheck,
    ) -> OwnedDerivedObjectReader {
        ObjectReader {
            control: OwnedReaderChecks {
                control,
                supplemental,
            },
            file: self.file.take(),
            directory: self.directory.take(),
            bytes: self.bytes,
            failure: self.failure.take(),
        }
    }
}
impl<C: ReaderCheck> ObjectReader<C> {
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
            self.control.checks().check()?;
            let length = out.len().min(65536);
            let count = self
                .file
                .as_mut()
                .expect("owned reader")
                .read(&mut out[..length])?;
            self.control.checks().check()?;
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
        let result = self.control.checks().check();
        self.remember(result)
    }
    pub fn seek_checked(&mut self, from: SeekFrom) -> Result<u64> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let result = (|| {
            self.control.checks().check()?;
            let position = self.file.as_mut().expect("owned reader").seek(from)?;
            self.control.checks().check()?;
            Ok(position)
        })();
        self.remember(result)
    }
    fn validate_kind_length(&self) -> Result<()> {
        self.control.checks().check()?;
        let metadata = self.file.as_ref().expect("owned reader").metadata()?;
        if !metadata.is_file() || metadata.len() != self.bytes {
            return Err(integrity("Referenced Data object kind or length differs"));
        }
        self.control.checks().check()
    }
    fn authenticate(&mut self, sha256: &str) -> Result<()> {
        self.validate_kind_length()?;
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
        join(result, cleanup, &self.control.checks())
    }
}
impl<C: ReaderCheck> Read for ObjectReader<C> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.read_checked(out).map_err(io::Error::other)
    }
}
impl<C: ReaderCheck> Seek for ObjectReader<C> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.seek_checked(from).map_err(io::Error::other)
    }
}
impl<C: ReaderCheck> Drop for ObjectReader<C> {
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
mod pinned_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod admitted_tests;
