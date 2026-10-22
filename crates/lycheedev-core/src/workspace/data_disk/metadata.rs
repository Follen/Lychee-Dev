//! Short quota capability for private Data SQLite. No SQL/producer callback is
//! exposed, and no new query owner or cross-domain metadata lock is acquired.
use super::*;
use std::ffi::OsStr;

pub(crate) struct MetadataQuota<'a> {
    owner: DerivedReservation<'a>,
}
impl<'a> MetadataQuota<'a> {
    pub(crate) fn acquire(
        home: &Path,
        control: &'a OperationControl,
        supplemental: &'a dyn Fn() -> Result<()>,
    ) -> Result<Self> {
        crate::workspace::open_controlled_with_check(home, control, supplemental)?;
        Ok(Self {
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
    pub(crate) fn path(&self) -> PathBuf {
        self.owner.paths.metadata.join("cache-v1.sqlite")
    }
    pub(crate) fn length(&self, name: &str) -> Result<Option<u64>> {
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
    pub(crate) fn admit_write(&mut self, journal_max: u64, page_bytes: u64) -> Result<u64> {
        if self.owner.metadata_read_only {
            return Err(ToolkitError::invalid(
                "read-only metadata owner cannot admit writes",
            ));
        }
        let main = self.length("cache-v1.sqlite")?.unwrap_or(0);
        let journal = self.length("cache-v1.sqlite-journal")?.unwrap_or(0);
        self.owner.report.existing = inventory_recovering(
            &self.owner.home,
            &self.owner.paths,
            &self.owner.control,
            None,
            &self.owner.directories,
        )?;
        let existing = self.owner.report.existing.total()?;
        let sidecars = sum([main, journal])?;
        let other = existing
            .checked_sub(sidecars)
            .ok_or_else(|| ToolkitError::invalid("private metadata accounting mismatch"))?;
        let journal_max = journal.max(journal_max);
        let fixed = sum([other, journal_max])?;
        let pages = self.owner.report.configured_max_bytes.saturating_sub(fixed) / page_bytes;
        // SQLite's fixed maximum, not an arbitrary per-request growth margin.
        let pages = pages.min(0xffff_fffe);
        let main_max = pages
            .checked_mul(page_bytes)
            .ok_or_else(|| ToolkitError::invalid("private metadata page overflow"))?;
        let extra = sum([
            main_max.saturating_sub(main),
            journal_max.saturating_sub(journal),
        ])?;
        self.owner.report.private_metadata_additional_peak = extra;
        self.owner.report.admitted_peak_bytes = sum([existing, extra])?;
        if pages < 2
            || main_max < main
            || self.owner.report.admitted_peak_bytes > self.owner.report.configured_max_bytes
        {
            return Err(budget_error(&self.owner.report));
        }
        self.owner.control.check()?;
        Ok(pages)
    }
    pub(crate) fn attempting_commit(&self, key: &str, generation: i64) -> usize {
        let mut receipts = self.owner.publication_receipts.borrow_mut();
        let index = receipts.len();
        receipts.push(serde_json::json!({
            "kind":"private-data-metadata", "state":"unknown",
            "path":self.path(), "key":key, "generation":generation,
        }));
        index
    }
    pub(crate) fn commit_state(&self, index: usize, state: &'static str) {
        self.owner.publication_receipts.borrow_mut()[index]["state"] = state.into();
    }
    pub(crate) fn prepare_receipt(&self) -> Result<()> {
        self.owner
            .publication_receipts
            .borrow_mut()
            .try_reserve(1)
            .map_err(|_| ToolkitError::invalid("cannot reserve metadata commit receipt"))
    }
    pub(crate) fn finish<T>(self, result: Result<T>) -> Result<T> {
        // Attempt inventory on errors too; a stopped inventory preserves its
        // exact stop and all files remain counted by the next admission.
        let verified = self.owner.verify_usage().map(|_| ());
        let result = crate::metadata::owned_write::finish_owned(result, verified);
        self.owner.finish_verified(result)
    }
}

/// Private read/recovery capability. It never lends its writer-shaped owner,
/// accepts growth or publishes receipts. Only its own namespace is inventoried:
/// unrelated missing shared bodies retain their existing optional-read behavior.
pub(crate) struct MetadataReadQuota<'a> {
    inner: MetadataQuota<'a>,
}
impl<'a> MetadataReadQuota<'a> {
    pub(crate) fn acquire(
        home: &Path,
        control: &'a OperationControl,
        supplemental: &'a dyn Fn() -> Result<()>,
    ) -> Result<Self> {
        crate::workspace::open_controlled_with_check(home, control, supplemental)?;
        let owner = DerivedReservation::acquire_scoped(
            None,
            home,
            crate::workspace::DEFAULT_DATA_CACHE_MAX_BYTES,
            AdditionalPeak::default(),
            control,
            Some(supplemental),
            true,
            true,
        )?;
        Ok(Self {
            inner: MetadataQuota { owner },
        })
    }
    pub(crate) fn path(&self) -> PathBuf {
        self.inner.path()
    }
    pub(crate) fn length(&self, name: &str) -> Result<Option<u64>> {
        self.inner.length(name)
    }
    pub(crate) fn verify_recovery(&self) -> Result<()> {
        self.inner.verify_recovery()
    }
    pub(crate) fn finish<T>(self, result: Result<T>) -> Result<T> {
        self.inner.finish(result)
    }
}
