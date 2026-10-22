//! In-memory service completion, separate from the fixed public wire schema.
use super::{AdmissionError, ProjectResult};

/// Exactly the three service outcomes. A completed call can project an ongoing
/// operation; it does not imply `ProjectResult::complete` or successful cleanup.
/// Verified reports and partially committed facts survive failures, including
/// checked-close errors. There is no success without a project result.
#[derive(Debug)]
#[must_use = "service outcomes retain committed facts and checked-close failures"]
pub enum ServiceOutcome {
    Completed(ProjectResult),
    Failed(AdmissionError),
    ReportedWithFailure {
        result: ProjectResult,
        error: AdmissionError,
    },
}
impl ServiceOutcome {
    pub(super) fn failure(error: impl Into<AdmissionError>) -> Self {
        Self::Failed(error.into())
    }
    pub(super) fn with_result(value: ProjectResult, result: Result<(), AdmissionError>) -> Self {
        match result {
            Ok(()) => Self::Completed(value),
            Err(error) => Self::ReportedWithFailure {
                result: value,
                error,
            },
        }
    }
    /// Consume the prior outcome so joining cleanup never needs a clone or an
    /// invalid placeholder. Preserve the original ordered error tree exactly.
    #[must_use]
    pub(super) fn join(self, result: Result<(), AdmissionError>) -> Self {
        let Err(error) = result else {
            return self;
        };
        match self {
            Self::Completed(result) => Self::ReportedWithFailure { result, error },
            Self::Failed(previous) => Self::Failed(AdmissionError::Joined(vec![previous, error])),
            Self::ReportedWithFailure {
                result,
                error: previous,
            } => Self::ReportedWithFailure {
                result,
                error: AdmissionError::Joined(vec![previous, error]),
            },
        }
    }
    /// Replace the projection only after the caller has observed/persisted newer
    /// authority facts. This cannot clear an earlier operation or close failure.
    pub(super) fn with_report(self, result: ProjectResult) -> Self {
        match self {
            Self::Completed(_) => Self::Completed(result),
            Self::Failed(error) | Self::ReportedWithFailure { error, .. } => {
                Self::ReportedWithFailure { result, error }
            }
        }
    }
    pub(super) fn report_mut(&mut self) -> Option<&mut ProjectResult> {
        match self {
            Self::Completed(result) | Self::ReportedWithFailure { result, .. } => Some(result),
            Self::Failed(_) => None,
        }
    }
    pub fn error(&self) -> Option<&AdmissionError> {
        match self {
            Self::Completed(_) => None,
            Self::Failed(error) | Self::ReportedWithFailure { error, .. } => Some(error),
        }
    }
    #[cfg(test)]
    pub(crate) fn into_report(self) -> Option<ProjectResult> {
        match self {
            Self::Completed(result) | Self::ReportedWithFailure { result, .. } => Some(result),
            Self::Failed(_) => None,
        }
    }
}

#[cfg(test)]
#[path = "outcome_tests.rs"]
mod tests;
