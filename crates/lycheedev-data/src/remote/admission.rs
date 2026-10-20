//! An admitted session is bound to a held workspace directory, never a second
//! caller-supplied path. The caller lends this owner exclusively to a session.
use crate::resource::Budget;
use lycheedev_core::{
    Result, ToolkitError,
    control::OperationControl,
    metadata::Metadata,
    storage::{Directory, FileControl, FileCost},
    workspace::data_resources::{self, DataResources, Policy},
};
use std::path::Path;

fn check(control: &OperationControl, budget: Option<&Budget>) -> Result<()> {
    control.check()?;
    budget.map_or(Ok(()), Budget::check)
}
struct Files<'a> {
    control: &'a OperationControl,
    budget: Option<&'a Budget>,
}
impl FileControl for Files<'_> {
    type Error = ToolkitError;
    fn charge(&mut self, _: FileCost) -> Result<()> {
        // These bounded mechanism costs are not Go logical/network work. Keep
        // both existing scopes effective without inventing public usage units.
        self.check()
    }
    fn check(&mut self) -> Result<()> {
        check(self.control, self.budget)
    }
}
fn same(a: &ToolkitError, b: &ToolkitError) -> bool {
    a.code == b.code
        && a.message == b.message
        && a.exit_code == b.exit_code
        && a.details == b.details
}
pub(crate) fn finish<T>(
    result: Result<T>,
    cleanup: impl IntoIterator<Item = ToolkitError>,
    control: Result<()>,
    budget: Result<()>,
) -> Result<T> {
    let (value, primary) = match result {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error)),
    };
    // Distinct close events survive, including identical error values. Only
    // repeated observations of the same sticky stop are deduplicated.
    let mut causes: Vec<_> = primary.into_iter().chain(cleanup).collect();
    for stop in control.err().into_iter().chain(budget.err()) {
        if !causes.iter().any(|previous| same(previous, &stop)) {
            causes.push(stop);
        }
    }
    let Some(mut first) = causes.first().cloned() else {
        return Ok(value.expect("successful admitted operation retains its value"));
    };
    if causes.len() > 1 {
        first.details =
            serde_json::json!({"schema":"lycheedev.admitted-workspace-errors.v1","causes":causes});
    }
    Err(first)
}

/// A held root, optionally with the fixed Go outer Metadata owner. This is not
/// permission to create a Session: only consuming acquire yields that type.
pub(super) struct OpenWorkspace<'a> {
    // On fallback Drop, the idle database closes before its held root.
    metadata: Option<Metadata>,
    directory: Option<Directory>,
    control: &'a OperationControl,
    budget: Option<&'a Budget>,
}
impl<'a> OpenWorkspace<'a> {
    fn open(
        home: &Path,
        control: &'a OperationControl,
        budget: Option<&'a Budget>,
    ) -> Result<Self> {
        check(control, budget)?;
        let directory = Directory::open_shared_controlled(home, &mut Files { control, budget })?;
        let owner = Self {
            metadata: None,
            directory: Some(directory),
            control,
            budget,
        };
        match owner.check() {
            Ok(()) => Ok(owner),
            Err(error) => owner.finish(Err(error)),
        }
    }
    pub fn open_metadata(
        home: &Path,
        control: &'a OperationControl,
        budget: Option<&'a Budget>,
    ) -> Result<Self> {
        let mut owner = Self::open(home, control, budget)?;
        // Preserve the two stop identities: Core checks invocation first, then
        // this supplemental Budget within every schema/SQLite wait attempt.
        let result = Metadata::open_controlled_with_check(owner.home(), true, control, || {
            budget.map_or(Ok(()), Budget::check)
        });
        match result {
            Ok(metadata) => {
                owner.metadata = Some(metadata);
                match owner.check() {
                    Ok(()) => Ok(owner),
                    Err(error) => owner.finish(Err(error)),
                }
            }
            Err(error) => owner.finish(Err(error)),
        }
    }
    pub fn acquire(self, policy: Policy) -> Result<AdmittedWorkspace<'a>> {
        let result = (|| {
            self.check()?;
            data_resources::acquire_with_check(self.home(), policy, self.control, || {
                check(self.control, self.budget)
            })
        })();
        match result {
            Ok(resources) => {
                let owner = AdmittedWorkspace {
                    resources: Some(resources),
                    workspace: self,
                };
                let initialized = (|| {
                    owner.check()?;
                    if let Some(budget) = owner.budget() {
                        budget.initialize_admitted(
                            owner.resources.as_ref().expect("live admission lease"),
                        )?;
                    }
                    owner.check()
                })();
                match initialized {
                    Ok(()) => Ok(owner),
                    Err(error) => owner.finish(Err(error)),
                }
            }
            Err(error) => self.finish(Err(error)),
        }
    }
    pub fn home(&self) -> &Path {
        self.directory
            .as_ref()
            .expect("live workspace directory")
            .path()
    }
    pub fn check(&self) -> Result<()> {
        check(self.control, self.budget)?;
        self.directory
            .as_ref()
            .expect("live workspace directory")
            .verify_controlled(&mut Files {
                control: self.control,
                budget: self.budget,
            })
    }
    pub fn finish<T>(self, result: Result<T>) -> Result<T> {
        self.finish_after(result, Vec::new())
    }
    fn finish_after<T>(self, result: Result<T>, cleanup: Vec<ToolkitError>) -> Result<T> {
        self.finish_after_metadata_close(result, cleanup, || Ok(()))
    }
    fn finish_after_metadata_close<T>(
        mut self,
        result: Result<T>,
        mut cleanup: Vec<ToolkitError>,
        after_close: impl FnOnce() -> Result<()>,
    ) -> Result<T> {
        if let Some(metadata) = self.metadata.take() {
            if let Err(error) = lycheedev_core::metadata::historical_write::finish_historical(
                metadata.close(),
                after_close(),
            ) {
                cleanup.push(error);
            }
        }
        if let Some(directory) = self.directory.take() {
            if let Err(errors) = directory.close() {
                cleanup.extend(errors.into_iter().map(ToolkitError::io));
            }
        }
        finish(
            result,
            cleanup,
            self.control.check(),
            self.budget.map_or(Ok(()), Budget::check),
        )
    }
}

pub(super) struct AdmittedWorkspace<'a> {
    // Fallback and checked teardown release slots before Metadata/root.
    resources: Option<DataResources>,
    workspace: OpenWorkspace<'a>,
}
impl<'a> AdmittedWorkspace<'a> {
    pub fn acquire(
        home: &Path,
        policy: Policy,
        control: &'a OperationControl,
        budget: Option<&'a Budget>,
    ) -> Result<Self> {
        // Standalone session callers retain their prior no-Metadata admission.
        OpenWorkspace::open(home, control, budget)?.acquire(policy)
    }
    pub fn home(&self) -> &Path {
        self.workspace.home()
    }
    pub fn control(&self) -> &'a OperationControl {
        self.workspace.control
    }
    pub fn budget(&self) -> Option<&'a Budget> {
        self.workspace.budget
    }
    pub(super) fn take_capture_metadata(&mut self) -> Metadata {
        self.workspace
            .metadata
            .take()
            .expect("query owns one metadata writer")
    }
    pub(super) fn restore_capture_metadata(&mut self, metadata: Metadata) {
        assert!(
            self.workspace.metadata.is_none(),
            "metadata owner must not be replaced"
        );
        self.workspace.metadata = Some(metadata);
    }
    pub fn reserve_derived_read<'reservation>(
        &'reservation self,
        check: &'reservation dyn Fn() -> Result<()>,
    ) -> Result<lycheedev_core::workspace::data_disk::DerivedReadLease<'reservation>> {
        self.check()?;
        self.resources
            .as_ref()
            .expect("live admission lease")
            .read_derived_cache_with_check(check)
    }
    pub fn reserve_derived_read_owned<'reservation>(
        &'reservation self,
        check: &'reservation lycheedev_core::workspace::data_disk::DerivedReadCheck,
    ) -> Result<lycheedev_core::workspace::data_disk::OwnedDerivedReadLease<'reservation>> {
        self.check()?;
        self.resources
            .as_ref()
            .expect("live admission lease")
            .read_derived_cache_owned_with_check(check)
    }
    pub fn reserve_derived_cache<'reservation>(
        &'reservation self,
        peak: lycheedev_core::workspace::data_disk::AdditionalPeak,
        check: &'reservation dyn Fn() -> Result<()>,
        budget: &'reservation crate::resource::Budget,
    ) -> Result<crate::generation::retirement::Admission<'reservation>> {
        self.check()?;
        self.resources
            .as_ref()
            .expect("live admission lease")
            .reserve_derived_cache_with_retirement(
                peak,
                check,
                |bytes| budget.reserve_retained(bytes),
                |key, bytes| crate::generation::retirement::reference(key, bytes, budget),
                |allocation| allocation.commit(),
            )
    }
    pub fn download_workers(&self) -> usize {
        self.resources
            .as_ref()
            .expect("live admission lease")
            .download_workers()
    }
    pub fn check(&self) -> Result<()> {
        self.workspace.check()
    }
    pub fn finish<T>(mut self, result: Result<T>) -> Result<T> {
        let mut cleanup = Vec::new();
        if let Some(resources) = self.resources.take() {
            cleanup.extend(resources.close_cleanup());
        }
        self.workspace.finish_after(result, cleanup)
    }
    #[cfg(test)]
    pub(super) fn finish_after_metadata_close<T>(
        mut self,
        result: Result<T>,
        after_close: &mut dyn FnMut() -> Result<()>,
    ) -> Result<T> {
        let mut cleanup = Vec::new();
        if let Some(resources) = self.resources.take() {
            cleanup.extend(resources.close_cleanup());
        }
        self.workspace
            .finish_after_metadata_close(result, cleanup, after_close)
    }
}

#[cfg(test)]
mod tests;
