//! Authenticate original DELETE-capable handles and keep them through cleanup.
use super::*;
pub use lycheedev_windows::filesystem::owned_cleanup::{AuthenticatedCleanup, OwnedTemporary};
use lycheedev_windows::filesystem::owned_cleanup::{CleanupCandidate, open_ancestor};
use std::os::windows::fs::MetadataExt;
pub fn authenticate_cleanup_file(
    path: &Path,
    bytes: u64,
    digest: &str,
    control: &crate::control::OperationControl,
) -> Result<AuthenticatedCleanup> {
    control.check()?;
    let candidate = CleanupCandidate::open(path, false)?;
    match candidate.authenticate::<ToolkitError>(|file| {
        let actual = hash_locked_file(file, bytes, || control.check())?;
        if file.metadata()?.len() != bytes || actual != digest {
            return Err(ToolkitError::invalid(
                "cleanup file differs from admitted inventory",
            ));
        }
        control.check()
    }) {
        Ok(held) => Ok(held),
        Err((error, candidate)) => finish_file(candidate.into_file(), Err(error), control),
    }
}
pub fn authenticate_cleanup_directory(
    path: &Path,
    control: &crate::control::OperationControl,
) -> Result<AuthenticatedCleanup> {
    control.check()?;
    let candidate = CleanupCandidate::open(path, true)?;
    match candidate.authenticate::<ToolkitError>(|_| control.check()) {
        Ok(held) => Ok(held),
        Err((error, candidate)) => finish_file(candidate.into_file(), Err(error), control),
    }
}
pub fn hold_cleanup_ancestor(
    path: &Path,
    control: &crate::control::OperationControl,
) -> Result<File> {
    control.check()?;
    let file = open_ancestor(path)?;
    let result = (|| {
        let info = file.metadata()?;
        if !info.is_dir() || info.file_attributes() & 0x400 != 0 {
            return Err(ToolkitError::invalid(
                "cleanup ancestry is redirected or not a directory",
            ));
        }
        control.check()
    })();
    match result {
        Ok(()) => Ok(file),
        Err(e) => finish_file(file, Err(e), control),
    }
}
