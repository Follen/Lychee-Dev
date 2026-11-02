//! Authoritative Data evidence database admission. This owner grants no cache
//! eviction authority and never exposes its database as rebuildable metadata.
//! Acquire before the common evidence writer gate; retain through checked close.
use super::*;
use crate::selection::selection_finish;
use std::ffi::OsStr;

pub(crate) struct EvidenceQuota<'a> {
    owner: DerivedReservation<'a>,
    main: Option<fs::File>,
}
impl<'a> EvidenceQuota<'a> {
    pub(crate) fn acquire(
        home: &Path,
        control: &'a OperationControl,
        supplemental: &'a dyn Fn() -> Result<()>,
    ) -> Result<Self> {
        crate::workspace::open_controlled_with_check(home, control, supplemental)?;
        Ok(Self {
            main: None,
            owner: DerivedReservation::acquire_inner(
                None,
                home,
                crate::workspace::DEFAULT_DATA_CACHE_MAX_BYTES,
                AdditionalPeak::default(),
                control,
                Some(supplemental),
                true,
            )?,
        })
    }
    pub(crate) fn acquire_read(
        home: &Path,
        control: &'a OperationControl,
        supplemental: &'a dyn Fn() -> Result<()>,
    ) -> Result<Self> {
        crate::workspace::open_controlled_with_check(home, control, supplemental)?;
        Ok(Self {
            main: None,
            owner: DerivedReservation::acquire_scoped(
                None,
                home,
                crate::workspace::DEFAULT_DATA_CACHE_MAX_BYTES,
                AdditionalPeak::default(),
                control,
                Some(supplemental),
                true,
                true,
            )?,
        })
    }
    pub(crate) fn publish_memory(
        &mut self,
        bytes: &[u8],
        maximum: usize,
        reference: &crate::evidence::BlobRef,
    ) -> Result<crate::evidence::BlobRef> {
        if self.owner.metadata_read_only {
            return Err(ToolkitError::invalid(
                "read-only evidence owner cannot publish bodies",
            ));
        }
        let home = self.owner.home.clone();
        super::shared_blobs::publish_prehashed_with_owner(
            &mut self.owner,
            &home,
            bytes,
            maximum,
            reference,
            None,
        )
    }
    pub(crate) fn enroll_verified(&mut self, blob: &crate::evidence::VerifiedBlob) -> Result<()> {
        if self.owner.metadata_read_only {
            return Err(ToolkitError::invalid(
                "read-only evidence owner cannot enroll bodies",
            ));
        }
        if std::path::absolute(blob.workspace())? != std::path::absolute(&self.owner.home)? {
            return Err(ToolkitError::invalid(
                "verified evidence belongs to another workspace",
            ));
        }
        super::shared_blobs::enroll_verified_with_owner(&mut self.owner, blob)
    }
    pub(crate) fn snapshot(&self) -> Result<ReservationReport> {
        let mut report = self.owner.report.clone();
        report.existing = self.owner.verify_usage()?;
        Ok(report)
    }
    pub(crate) fn path(&self) -> PathBuf {
        self.owner.paths.metadata.join("evidence-v2.sqlite")
    }
    pub(crate) fn lock_directory(&self) -> &Directory {
        self.owner
            .directories
            .first()
            .expect("held quota locks directory")
    }
    /// Pin the original main file before SQLite sees its path. Absence is a
    /// read-only miss: neither inspection nor inventory creates a database.
    pub(crate) fn pin_existing_main(&mut self) -> Result<Option<u64>> {
        self.owner.control.check()?;
        if self.main.is_none() {
            let directory = self
                .owner
                .directories
                .iter()
                .find(|d| d.path() == self.owner.paths.metadata)
                .expect("held private metadata directory");
            self.main = directory.open_optional_pinned_inspection_file_controlled(
                OsStr::new("evidence-v2.sqlite"),
                &mut Controlled(&self.owner.control),
            )?;
        }
        // Install the owner before validation/cancellation; finish always uses
        // checked close, including reparse rejection and cancelled opens.
        let result = self.main.as_ref().map(main_length).transpose();
        selection_finish(result, self.owner.control.check())
    }
    /// Admission precedes even native creation of the empty SQLite main. Keep
    /// this owner through every connection close; OPEN_IF never truncates.
    pub(crate) fn admit_and_pin_main(
        &mut self,
        journal_bound: fn(u64) -> Result<u64>,
        page_bytes: u64,
    ) -> Result<()> {
        self.admit_write(journal_bound, page_bytes)?;
        if self.main.is_none() {
            let directory = self
                .owner
                .directories
                .iter()
                .find(|d| d.path() == self.owner.paths.metadata)
                .expect("held private metadata directory");
            self.main = Some(directory.open_or_create_pinned_inspection_file_controlled(
                OsStr::new("evidence-v2.sqlite"),
                &mut Controlled(&self.owner.control),
            )?);
        }
        let result = main_length(self.main.as_ref().expect("owned admitted main")).map(|_| ());
        selection_finish(result, self.owner.control.check())?;
        // OPEN_IF may have won an externally inserted regular file after the
        // absent observation. Re-admit its pinned length before SQLite can
        // recover/read schema or write; never trust the earlier absent size.
        self.admit_write(journal_bound, page_bytes).map(|_| ())
    }
    pub(crate) fn length(&self, name: &str) -> Result<Option<u64>> {
        if name == "evidence-v2.sqlite" {
            if let Some(file) = &self.main {
                self.owner.control.check()?;
                return selection_finish(main_length(file).map(Some), self.owner.control.check());
            }
        }
        let directory = self
            .owner
            .directories
            .iter()
            .find(|d| d.path() == self.owner.paths.metadata)
            .expect("held private metadata directory");
        let Some(file) = directory.open_optional_inspection_file_controlled(
            OsStr::new(name),
            &mut Controlled(&self.owner.control),
        )?
        else {
            return Ok(None);
        };
        let result = (|| {
            self.owner.control.check()?;
            let metadata = file.metadata()?;
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err(ToolkitError::invalid(
                        "private Data metadata entry is a reparse point",
                    ));
                }
            }
            if !metadata.is_file() {
                return Err(ToolkitError::invalid(
                    "private Data metadata entry is not a regular file",
                ));
            }
            Ok(Some(metadata.len()))
        })();
        finish_file(file, result, &self.owner.control)
    }
    /// Normal managed hot recovery rewrites/truncates the existing main file.
    /// This zero-growth owner may read/recover an already over-limit home, but
    /// must explicitly admit every new transaction's main+journal peak below.
    pub(crate) fn verify_recovery(&self) -> Result<()> {
        self.owner.verify_usage().map(|_| ())
    }
    pub(crate) fn admit_write(
        &mut self,
        journal_bound: fn(u64) -> Result<u64>,
        page_bytes: u64,
    ) -> Result<u64> {
        if self.owner.metadata_read_only {
            return Err(ToolkitError::invalid(
                "read-only metadata owner cannot admit writes",
            ));
        }
        let main = self.length("evidence-v2.sqlite")?.unwrap_or(0);
        let journal = self.length("evidence-v2.sqlite-journal")?.unwrap_or(0);
        self.owner.report.existing = inventory_recovering(
            &self.owner.home,
            &self.owner.paths,
            &self.owner.control,
            None,
            &self.owner.directories,
        )?;
        let existing = self.owner.report.existing.total()?;
        let other = existing
            .checked_sub(sum([main, journal])?)
            .ok_or_else(|| ToolkitError::invalid("private metadata accounting mismatch"))?;
        // A removal scope may perform several transactions. Bound rollback
        // journals by the admitted maximum database, not its initial size.
        // Thus a successful first COMMIT cannot invalidate the next admission.
        let available = self.owner.report.configured_max_bytes.saturating_sub(other);
        let mut low = 0u64;
        let mut high = (available / page_bytes).min(0xffff_fffe);
        while low < high {
            self.owner.control.check()?;
            let candidate = low + (high - low + 1) / 2;
            let main_max = candidate
                .checked_mul(page_bytes)
                .ok_or_else(|| ToolkitError::invalid("private metadata page overflow"))?;
            if sum([main_max, journal.max(journal_bound(main_max)?)])? <= available {
                low = candidate;
            } else {
                high = candidate - 1;
            }
        }
        let pages = low;
        let main_max = pages
            .checked_mul(page_bytes)
            .ok_or_else(|| ToolkitError::invalid("private metadata page overflow"))?;
        let journal_max = journal.max(journal_bound(main_max)?);
        let extra = sum([
            main_max.saturating_sub(main),
            journal_max.saturating_sub(journal),
        ])?;
        self.owner.report.private_metadata_additional_peak = extra;
        self.owner.report.admitted_peak_bytes = sum([existing, extra])?;
        if pages < 3
            || main_max < main
            || self.owner.report.admitted_peak_bytes > self.owner.report.configured_max_bytes
        {
            return Err(budget_error(&self.owner.report));
        }
        self.owner.control.check()?;
        Ok(pages)
    }
    pub(crate) fn finish<T>(self, result: Result<T>) -> Result<T> {
        // Attempt inventory on errors too; a stopped inventory preserves its
        // exact stop and all files remain counted by the next admission.
        let verified = self.owner.verify_usage().map(|_| ());
        let result = crate::metadata::owned_write::finish_owned(result, verified);
        // with_catalog closes every SQLite connection before owner.finish.
        // Keep directory/quota ownership until the main leaf close completes.
        let result = match self.main {
            Some(file) => finish_file(file, result, &self.owner.control),
            None => result,
        };
        self.owner.finish_verified(result)
    }
}
fn main_length(file: &fs::File) -> Result<u64> {
    let metadata = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(ToolkitError::invalid(
                "private Data main is a reparse point",
            ));
        }
    }
    if !metadata.is_file() {
        return Err(ToolkitError::invalid(
            "private Data main is not a regular file",
        ));
    }
    Ok(metadata.len())
}
