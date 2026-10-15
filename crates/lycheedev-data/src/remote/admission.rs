//! An admitted session is bound to a held workspace directory, never a second
//! caller-supplied path. The caller lends this owner exclusively to a session.
use crate::resource::Budget;
use lycheedev_core::{
    Result, ToolkitError,
    control::OperationControl,
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
fn finish<T>(
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

pub(super) struct AdmittedWorkspace<'a> {
    // Fallback field drop order releases slots before directory handles. Normal
    // consuming finish explicitly closes every owner and retains diagnostics.
    resources: Option<DataResources>,
    directory: Option<Directory>,
    control: &'a OperationControl,
    budget: Option<&'a Budget>,
}
impl<'a> AdmittedWorkspace<'a> {
    pub fn acquire(
        home: &Path,
        policy: Policy,
        control: &'a OperationControl,
        budget: Option<&'a Budget>,
    ) -> Result<Self> {
        check(control, budget)?;
        let directory = Directory::open_shared_controlled(home, &mut Files { control, budget })?;
        let mut owner = Self {
            resources: None,
            directory: Some(directory),
            control,
            budget,
        };
        let result = data_resources::acquire_with_check(owner.home(), policy, control, || {
            check(control, budget)
        });
        match result {
            Ok(resources) => {
                owner.resources = Some(resources);
                match owner.check() {
                    Ok(()) => Ok(owner),
                    Err(error) => owner.finish(Err(error)),
                }
            }
            Err(error) => owner.finish(Err(error)),
        }
    }
    pub fn home(&self) -> &Path {
        self.directory
            .as_ref()
            .expect("live admitted directory")
            .path()
    }
    pub fn control(&self) -> &'a OperationControl {
        self.control
    }
    pub fn budget(&self) -> Option<&'a Budget> {
        self.budget
    }
    pub fn download_workers(&self) -> usize {
        self.resources
            .as_ref()
            .expect("live admission lease")
            .download_workers()
    }
    pub fn check(&self) -> Result<()> {
        check(self.control, self.budget)?;
        self.directory
            .as_ref()
            .expect("live admitted directory")
            .verify_controlled(&mut Files {
                control: self.control,
                budget: self.budget,
            })
    }
    pub fn finish<T>(mut self, result: Result<T>) -> Result<T> {
        let mut cleanup = Vec::new();
        if let Some(resources) = self.resources.take() {
            if let Err(error) = resources.close_with_check(|| check(self.control, self.budget)) {
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

#[cfg(test)]
mod tests;
