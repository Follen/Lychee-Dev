//! Existing historical main-file identity, held through SQLite checked close.
//! Sidecars are inspected without a delete-denying pin: SQLite owns their lifecycle.
use super::*;
use crate::storage::{self, Directory, FileControl, FileCost};
use std::{ffi::OsStr, fs::File};
struct Checks<'a>(&'a OperationControl, &'a dyn Fn() -> Result<()>);
impl FileControl for Checks<'_> {
    type Error = ToolkitError;
    fn check(&mut self) -> Result<()> {
        self.0.check()?;
        (self.1)()
    }
    fn charge(&mut self, _: FileCost) -> Result<()> {
        self.check()
    }
}
pub(super) struct Anchor {
    directory: Directory,
    main: Option<File>,
}
fn validate(file: &File) -> Result<()> {
    let metadata = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(integrity());
        }
    }
    if !metadata.is_file() {
        return Err(integrity());
    }
    Ok(())
}
impl Anchor {
    pub(super) fn open(
        home: &Path,
        control: &OperationControl,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<Self> {
        let mut checks = Checks(control, check);
        checks.check()?;
        let directory = Directory::open_shared_controlled(&home.join("state"), &mut checks)?;
        let mut anchor = Self {
            directory,
            main: None,
        };
        let result = (|| {
            anchor.main = anchor
                .directory
                .open_optional_pinned_inspection_file_controlled(
                    OsStr::new("toolkit.sqlite"),
                    &mut checks,
                )?;
            if let Some(file) = &anchor.main {
                validate(file)?;
            }
            for name in [
                "toolkit.sqlite-wal",
                "toolkit.sqlite-shm",
                "toolkit.sqlite-journal",
            ] {
                if let Some(file) = anchor
                    .directory
                    .open_optional_inspection_file_controlled(OsStr::new(name), &mut checks)?
                {
                    let result = validate(&file);
                    finish_owned(result, storage::close_file(file).map_err(Into::into))?;
                }
            }
            checks.check()
        })();
        match result {
            Ok(()) => Ok(anchor),
            Err(error) => finish_owned(Err(error), anchor.close()),
        }
    }
    pub(super) fn ensure_main(
        &mut self,
        control: &OperationControl,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<()> {
        if self.main.is_none() {
            self.main = Some(
                self.directory
                    .open_or_create_pinned_inspection_file_controlled(
                        OsStr::new("toolkit.sqlite"),
                        &mut Checks(control, check),
                    )?,
            );
            validate(self.main.as_ref().expect("new main owner"))?;
        }
        control.check()?;
        check()
    }
    pub(super) fn present(&self) -> bool {
        self.main.is_some()
    }
    pub(super) fn close(self) -> Result<()> {
        let mut result = self
            .main
            .map_or(Ok(()), |file| storage::close_file(file).map_err(Into::into));
        if let Err(errors) = self.directory.close() {
            for error in errors {
                result = finish_owned(result, Err(error.into()));
            }
        }
        result
    }
}
