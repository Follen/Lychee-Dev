//! Explicit use/acquisition markers, not filesystem automatic access times.
//! Zero-payload atomic replacement changes only advisory eviction order. It
//! cannot authorize body deletion, change immutable content or consume payload
//! allowance in an over-limit read. This is logical payload accounting, not
//! zero filesystem cost: each marker consumes directory/inode metadata and an
//! inventoried file entry. Inventory bounds total entries, and retirement
//! removes live and crash-orphan markers. Wall-clock rollback affects priority
//! only; equal timestamps use SHA ordering. No marker grants deletion rights.
use super::*;
use lycheedev_windows::filesystem::destination::StagedFile;
use std::{ffi::OsStr, time::SystemTime};

impl DerivedReservation<'_> {
    pub(super) fn object_recency(&self, sha: &str) -> Result<Option<SystemTime>> {
        self.paths.object(sha)?;
        let path = self.paths.recency();
        let directory = self
            .directories
            .iter()
            .find(|d| d.path() == path)
            .expect("held recency directory");
        let Some(file) = directory
            .open_optional_file_controlled(OsStr::new(sha), &mut Controlled(&self.control))?
        else {
            return Ok(None);
        };
        let result = (|| {
            self.control.check()?;
            let metadata = file.metadata()?;
            if !metadata.is_file() || metadata.len() != 0 {
                return Err(ToolkitError::invalid(
                    "Data recency marker is not an empty regular file",
                ));
            }
            Ok(Some(metadata.modified()?))
        })();
        finish_file(file, result, &self.control)
    }
    pub(super) fn touch_object(&self, sha: &str) -> Result<()> {
        self.touch_object_bounded(sha, MAX_ENTRIES / 4)
    }
    pub(super) fn touch_object_bounded(&self, sha: &str, maximum: usize) -> Result<()> {
        // Validate any existing target before replacement. StagedFile's same-
        // directory handle rename never opens/writes through a replacement leaf.
        self.object_recency(sha)?;
        let path = self.paths.recency();
        let directory = self
            .directories
            .iter()
            .find(|d| d.path() == path)
            .expect("held recency directory");
        // Advisory metadata must not grow without bound across failed cleanup
        // or repeated crash windows. At saturation preserve existing history
        // and skip a touch; retirement can remove cold/orphan markers. Never
        // fail a content read merely because the optional history is full.
        let mut entries = 0usize;
        for entry in fs::read_dir(directory.path())? {
            self.control.check()?;
            entry?;
            entries += 1;
            if entries >= maximum {
                return Ok(());
            }
        }
        let mut stage = StagedFile::create(directory, &mut Controlled(&self.control))?;
        let result = (|| {
            stage.seal_rebuildable_cache()?;
            self.control.check()?;
            stage.replace_name(OsStr::new(sha), &mut Controlled(&self.control))?;
            self.control.check()
        })();
        join(
            result,
            stage.close().into_iter().map(ToolkitError::io),
            &self.control,
        )
    }
}
