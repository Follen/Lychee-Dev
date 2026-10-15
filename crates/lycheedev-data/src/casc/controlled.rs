//! The local archive's original invocation and optional independent budget.
//! Ordinary legacy entrypoints retain their raw-token/limits checks.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct Scope<'a> {
    pub limits: Limits,
    pub cancel: Option<&'a AtomicBool>,
    pub control: Option<&'a OperationControl>,
    pub budget: Option<&'a Budget>,
}
impl Scope<'_> {
    pub fn check(self) -> Result<()> {
        if self.control.is_none() {
            check(self.limits, self.cancel)?;
        }
        if let Some(control) = self.control {
            control.check()?;
        }
        if let Some(budget) = self.budget {
            budget.check()?;
        }
        check(self.limits, self.cancel)
    }
    pub fn guard(self, root: &Path, path: &Path) -> Result<()> {
        storage::guard_path_with_check(root, path, || self.check())
    }
    pub fn finish<T>(self, result: Result<T>, cleanup: Vec<ToolkitError>) -> Result<T> {
        let checks = [
            self.control.and_then(|control| control.check().err()),
            self.budget.and_then(|budget| budget.check().err()),
            check(self.limits, self.cancel).err(),
        ];
        finish(result, cleanup, checks.into_iter().flatten())
    }
    pub fn close<T>(self, file: fs::File, result: Result<T>) -> Result<T> {
        self.finish(
            result,
            storage::close_file(file)
                .err()
                .map(Into::into)
                .into_iter()
                .collect(),
        )
    }
}
impl storage::FileControl for Scope<'_> {
    type Error = ToolkitError;
    fn charge(&mut self, _: storage::FileCost) -> Result<()> {
        (*self).check()
    }
    fn check(&mut self) -> Result<()> {
        (*self).check()
    }
}

pub(super) fn finish<T>(
    result: Result<T>,
    cleanup: Vec<ToolkitError>,
    checks: impl IntoIterator<Item = ToolkitError>,
) -> Result<T> {
    let (value, primary) = match result {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error)),
    };
    let mut stops = Vec::new();
    for stop in checks {
        // Only repeated observations of the same stop are coalesced. Each
        // real close invocation remains an event, including identical errors.
        if primary
            .iter()
            .chain(stops.iter())
            .any(|old: &ToolkitError| {
                old.code == stop.code
                    && old.message == stop.message
                    && old.exit_code == stop.exit_code
                    && old.details == stop.details
            })
        {
            continue;
        }
        stops.push(stop);
    }
    let Some(mut first) = primary
        .iter()
        .chain(cleanup.iter())
        .chain(stops.iter())
        .next()
        .cloned()
    else {
        return Ok(value.expect("successful CASC result"));
    };
    if usize::from(primary.is_some()) + cleanup.len() + stops.len() > 1 {
        first.details = serde_json::json!({"schema":"lycheedev.casc-cleanup.v1", "primary":primary, "cleanup":cleanup, "stops":stops});
    }
    Err(first)
}

#[cfg(test)]
mod tests;
