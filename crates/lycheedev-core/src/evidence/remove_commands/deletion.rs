//! The destructive step consumes only the authenticated original file handle.
//! Required ancestry is held; only native optional entry acquisition is a miss.
use super::*;
use crate::storage::Directory;
use bounded::lifecycle::PathControl;
fn finish_directory<T>(mut result: Result<T>, directory: Directory) -> Result<T> {
    if let Err(errors) = directory.close() {
        for error in errors {
            result = finish_owned(result, Err(error.into()));
        }
    }
    result
}
pub(super) fn remove(
    home: &Path,
    capture: &CaptureRef,
    control: &OperationControl,
    disposition: &mut Option<&'static str>,
) -> Result<()> {
    control.check()?;
    let (digest, bytes) = capture.blob_identity();
    if !valid_digest(digest, 64) {
        return Err(ToolkitError::new(
            "vault.blob_integrity",
            "vault.blob_integrity",
            4,
        ));
    }
    let path = blob_path(home, digest)?;
    storage::guard_path_with_check(home, &path, || control.check())?;
    let parent = Directory::open_shared_controlled(
        &home.join("blobs"),
        &mut PathControl {
            control: Some(control),
            supplemental: &mut || Ok(()),
        },
    )?;
    let result = (|| {
        let shard = path
            .parent()
            .expect("blob shard")
            .file_name()
            .expect("shard name");
        let file = parent.open_optional_file_controlled(
            shard,
            &mut PathControl {
                control: Some(control),
                supplemental: &mut || Ok(()),
            },
        )?;
        let Some(file) = file else {
            *disposition = Some("missing");
            return control.check();
        };
        // Keep the original shard entry while the strict directory owner is
        // acquired. A later walk failure is never reclassified as NotFound.
        let opened = Directory::open_controlled(
            &parent.path().join(shard),
            &mut PathControl {
                control: Some(control),
                supplemental: &mut || Ok(()),
            },
        );
        let closed = storage::close_file(file).map_err(ToolkitError::from);
        let directory = match opened {
            Ok(directory) => match crate::selection::selection_finish(closed, control.check()) {
                Ok(()) => directory,
                Err(error) => return finish_directory(Err(error), directory),
            },
            Err(error) => {
                return crate::selection::selection_finish(
                    finish_owned(Err(error), closed),
                    control.check(),
                );
            }
        };
        let result = (|| {
            let held =
                storage::authenticate_cleanup_file_optional(&path, bytes as u64, digest, control)?;
            let Some(held) = held else {
                *disposition = Some("missing");
                return control.check();
            };
            if let Err(error) = control.check() {
                return storage::finish_file(held.into_file(), Err(error), control);
            }
            let (file, removed) = held.discard();
            *disposition = Some(if removed.is_ok() {
                "delete_requested"
            } else {
                "unknown"
            });
            let closed = storage::close_file(file).map_err(ToolkitError::from);
            if removed.is_ok() && closed.is_ok() {
                *disposition = Some("deleted");
            }
            // A successful disposition is retained even if close/control fails.
            #[cfg(test)]
            let closed = finish_owned(closed, super::tests::after_blob_close());
            crate::selection::selection_finish(
                finish_owned(removed.map_err(Into::into), closed),
                control.check(),
            )
        })();
        finish_directory(result, directory)
    })();
    crate::selection::selection_finish(finish_directory(result, parent), control.check())
}
