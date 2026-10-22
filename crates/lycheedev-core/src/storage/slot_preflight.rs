//! Best-effort sharing preflight for strict-member slot directories only.
//! Detect an already-held deny-delete handle before closing our original temp.
//! Other open errors (including access denied or absence) do not add a new
//! publication permission gate. Races after the probe, uncertain close, late
//! cancellation and historical residues still use conservative retention.
use super::*;

pub(super) enum Failure {
    Sharing(ToolkitError),
    Close(ToolkitError),
}

/// Slot files use the same bounded same-directory writer and durable replace,
/// with an optional observation before original-temp close. This is not a
/// reservation or a recovery capability; callers must still treat writer
/// errors conservatively, and must never adopt/remove unknown temp residues.
pub fn slot_atomic_write_controlled(
    path: &Path,
    bytes: &[u8],
    control: &crate::control::OperationControl,
) -> Result<()> {
    atomic_write_with_hooks(
        path,
        bytes,
        || control.check(),
        lycheedev_windows::filesystem::close_file,
        |path| {
            check_with(
                path,
                lycheedev_windows::filesystem::sharing_probe::open_delete_sharing_probe,
                lycheedev_windows::filesystem::close_file,
            )
        },
        owned_cleanup::OwnedTemporary::discard,
        lycheedev_windows::durable_replace,
    )
}

fn error(path: &Path, stage: &str, cause: std::io::Error) -> ToolkitError {
    let raw = cause.raw_os_error();
    ToolkitError::io(cause).with_details(serde_json::json!({
        "schema": "lycheedev.slot-sharing-preflight.v1",
        "stage": stage,
        "target": path,
        "rawOsError": raw,
        "renameAttempted": false,
        "published": false,
    }))
}

fn check_with(
    path: &Path,
    open: impl FnOnce(&Path) -> std::io::Result<File>,
    close: impl FnOnce(File) -> std::io::Result<()>,
) -> std::result::Result<(), Failure> {
    match open(path) {
        // Classify the raw Win32 code, not ErrorKind, text or a list of vaguely
        // related lock/access errors. No handle exists on this failure branch.
        Err(cause) if cause.raw_os_error() == Some(32) => {
            Err(Failure::Sharing(error(path, "open", cause)))
        }
        Err(_) => Ok(()),
        Ok(file) => close(file).map_err(|cause| Failure::Close(error(path, "close", cause))),
    }
}

#[cfg(test)]
#[path = "slot_preflight_tests.rs"]
mod tests;
