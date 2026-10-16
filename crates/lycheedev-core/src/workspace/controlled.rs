//! Marker reads retain the original operation and supplemental budget at every
//! bounded I/O boundary. The historical unbounded document contract is retained.
use super::*;
use crate::control::OperationControl;
use std::io::Read;
pub fn open_controlled(home: &Path, control: &OperationControl) -> Result<Value> {
    open_controlled_with_check(home, control, || Ok(()))
}
pub fn open_controlled_with_check(
    home: &Path,
    control: &OperationControl,
    mut supplemental: impl FnMut() -> Result<()>,
) -> Result<Value> {
    let result = read_with_check(home, control, &mut supplemental, &mut false);
    let result = selection::selection_finish(result, control.check());
    // Supplemental cancellation is independently observable at terminal cleanup,
    // even when the original invocation has also stopped.
    selection::selection_finish(result, supplemental())
}
pub(crate) fn inspect_controlled(home: &Path, control: &OperationControl) -> (Result<Value>, bool) {
    let mut missing = false;
    let result = read_with_check(home, control, &mut || Ok(()), &mut missing);
    (result, missing)
}
fn read_with_check(
    home: &Path,
    control: &OperationControl,
    supplemental: &mut impl FnMut() -> Result<()>,
    missing: &mut bool,
) -> Result<Value> {
    let mut check = || {
        control.check()?;
        supplemental()
    };
    check()?;
    let mut file = match fs::File::open(home.join("workspace.json")) {
        Ok(file) => file,
        Err(error) => {
            let result = if error.kind() == std::io::ErrorKind::NotFound {
                // Match OpenStore's missing-marker distinction. A failed
                // directory enumeration is not evidence of a legacy workspace.
                let occupied = (|| -> Result<bool> {
                    check()?;
                    let entries = match fs::read_dir(home) {
                        Ok(v) => v,
                        Err(_) => return Ok(false),
                    };
                    let mut any = false;
                    for entry in entries {
                        check()?;
                        if entry.is_err() {
                            return Ok(false);
                        }
                        any = true;
                    }
                    check()?;
                    Ok(any)
                })()?;
                if occupied {
                    Err(ToolkitError::new(
                        "vault.legacy_detected",
                        "vault.legacy_detected",
                        3,
                    ))
                } else {
                    *missing = true;
                    Err(error.into())
                }
            } else {
                Err(error.into())
            };
            return selection::selection_finish(result, check());
        }
    };
    let result = (|| {
        let mut raw = Vec::new();
        let mut buffer = [0; 65536];
        loop {
            check()?;
            let n = file.read(&mut buffer)?;
            raw.extend_from_slice(&buffer[..n]);
            check()?;
            if n == 0 {
                break;
            }
        }
        let mut callback_failed = false;
        let decoded = crate::evidence::decode_workspace_identity_with_check(&raw, || {
            let result = check();
            if result.is_err() {
                callback_failed = true;
            }
            result
        });
        let result = decoded
            .map_err(|error| {
                if !callback_failed {
                    unsupported_identity()
                } else {
                    error
                }
            })
            .and_then(validate_identity);
        selection::selection_finish(result, check())
    })();
    let result = storage::finish_file(file, result, control);
    selection::selection_finish(result, check())
}
#[cfg(test)]
mod tests;
