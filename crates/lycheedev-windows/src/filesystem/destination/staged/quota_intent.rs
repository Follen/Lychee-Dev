//! Restricted quota bookkeeping, never a general published-body deletion API.
//! Only this constructor can create the capability; arbitrary StagedFile/File
//! values cannot be converted. The original CREATE_NEW handle is retained until
//! checked close or explicit withdrawal after the higher-level writer settles.
use super::*;

pub struct QuotaIntent<'a> {
    stage: StagedFile<'a>,
    attempted: bool,
}
impl<'a> QuotaIntent<'a> {
    pub fn create<C: FileControl>(
        parent: &'a Directory,
        control: &mut C,
    ) -> Result<Self, C::Error> {
        let path = parent.path();
        if path.file_name() != Some(std::ffi::OsStr::new("shared-blobs-v1"))
            || path.parent().and_then(Path::file_name)
                != Some(std::ffi::OsStr::new("data-derived-v1"))
            || path
                .parent()
                .and_then(Path::parent)
                .and_then(Path::file_name)
                != Some(std::ffi::OsStr::new("indexes"))
        {
            return Err(
                invalid("quota intent requires its fixed versioned bookkeeping directory").into(),
            );
        }
        StagedFile::create(parent, control).map(|stage| Self {
            stage,
            attempted: false,
        })
    }
    pub fn name(&self) -> &str {
        self.stage.name()
    }
    pub fn publication(&self) -> StagedPublication {
        self.stage.publication()
    }
    /// Fixed-size private record. Its name is derived from the embedded SHA;
    /// no caller-supplied pathname, body, or overwrite authority is accepted.
    pub fn publish_record<C: FileControl>(
        &mut self,
        record: &[u8; 48],
        control: &mut C,
    ) -> Result<StagedPublication, C::Error> {
        self.publish_record_using(record, control, File::sync_all, |stage, name| {
            stage.rename(name, false)
        })
    }
    fn publish_record_using<C: FileControl>(
        &mut self,
        record: &[u8; 48],
        control: &mut C,
        sync: impl FnOnce(&File) -> io::Result<()>,
        rename: impl FnOnce(&mut Stage, &Path) -> io::Result<()>,
    ) -> Result<StagedPublication, C::Error> {
        if std::mem::replace(&mut self.attempted, true) {
            return Err(invalid("quota intent publication was already attempted").into());
        }
        if &record[..8] != b"LDSB0001"
            || u64::from_le_bytes(record[8..16].try_into().expect("fixed bytes")) > (1 << 40)
        {
            return Err(invalid("invalid quota-intent record").into());
        }
        let mut name = String::with_capacity(71);
        use std::fmt::Write as _;
        for byte in &record[16..] {
            write!(&mut name, "{byte:02x}").expect("String write");
        }
        name.push_str(".intent");
        control.check()?;
        self.stage.write_all(record).map_err(C::Error::from)?;
        control.check()?;
        self.stage.seal_with(sync).map_err(C::Error::from)?;
        control.check()?;
        self.stage
            .publish_name_with(std::ffi::OsStr::new(&name), control, rename)
    }
    /// Ordinary close preserves Published/Unknown and discards only a private
    /// stage, exactly like StagedFile. No implicit withdraw happens in Drop.
    pub fn close(self) -> Vec<io::Error> {
        self.stage.close()
    }
    /// Caller must first prove its body was NotPublished and all body stages
    /// closed cleanly. This revokes only this newly created quota intent using
    /// its original handle. Unknown never grants withdrawal authority.
    pub fn withdraw(self) -> Vec<io::Error> {
        self.withdraw_with(super::super::super::close_file)
    }
    fn withdraw_with(mut self, close: impl FnOnce(File) -> io::Result<()>) -> Vec<io::Error> {
        if self.stage.publication == StagedPublication::Unknown {
            let mut errors = vec![invalid(
                "unknown quota-intent publication cannot be withdrawn",
            )];
            errors.extend(self.stage.close_with(close));
            return errors;
        }
        if self.stage.publication == StagedPublication::Published {
            // Only this versioned, fixed-record capability can revoke its own
            // known publication. Reuse the same disposition/close mechanism;
            // the original File has never escaped or been reopened by name.
            self.stage.publication = StagedPublication::NotPublished;
            self.stage.stage.published = false;
        }
        self.stage.close_with(close)
    }
}

#[cfg(test)]
mod tests;
