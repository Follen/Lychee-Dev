//! Actual Data automatic/project pin resolution with the original invocation
//! and an independent supplemental budget at file and SQLite wait boundaries.
use super::*;
use crate::control::OperationControl;
use std::{ffi::OsStr, io::Read};
mod management;
mod wire;
pub(super) use management::{dispatch, inspect_value, optional};

fn error(code: &str) -> ToolkitError {
    ToolkitError::new(
        code,
        code,
        if matches!(
            code,
            "selection.project_format" | "selection.project_conflict"
        ) {
            4
        } else {
            3
        },
    )
}
fn format() -> ToolkitError {
    error("selection.project_format")
}
fn pure(error: &ToolkitError, code: &str) -> bool {
    error.code == code && error.details.is_null()
}
struct Scope<'a, C> {
    control: &'a OperationControl,
    supplemental: C,
}
impl<C: FnMut() -> Result<()>> Scope<'_, C> {
    fn check(&mut self) -> Result<()> {
        self.control.check()?;
        (self.supplemental)()
    }
    fn finish<T>(
        &mut self,
        result: Result<T>,
        cleanup: impl IntoIterator<Item = ToolkitError>,
    ) -> Result<T> {
        let result = cleanup.into_iter().fold(result, |result, error| {
            crate::metadata::historical_write::finish_historical(result, Err(error))
        });
        let result = selection_finish(result, self.control.check());
        selection_finish(result, (self.supplemental)())
    }
}
impl<C: FnMut() -> Result<()>> storage::FileControl for Scope<'_, C> {
    type Error = ToolkitError;
    fn charge(&mut self, _: storage::FileCost) -> Result<()> {
        self.check()
    }
    fn check(&mut self) -> Result<()> {
        Scope::check(self)
    }
}

// filepath.Abs/Clean is lexical: do not resolve symlinks as canonicalize would.
fn absolute(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut clean = PathBuf::new();
    for part in absolute.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                clean.pop();
            }
            other => clean.push(other.as_os_str()),
        }
    }
    Ok(clean)
}

/// Same nearest-declaration choice as Go FindProject: a malformed nearer file
/// is selected, and only actual NotFound continues to the next ancestor.
fn find<C: FnMut() -> Result<()>>(start: &Path, scope: &mut Scope<'_, C>) -> Result<PathBuf> {
    scope.check()?;
    let absolute = absolute(start)?;
    for parent in absolute.ancestors() {
        scope.check()?;
        match fs::symlink_metadata(parent.join("lycheedev.json")) {
            Ok(_) => {
                scope.check()?;
                return Ok(parent.to_owned());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => scope.check()?,
            Err(error) => return Err(error.into()),
        }
    }
    Err(error("selection.project_missing"))
}
pub(super) fn find_controlled(start: &Path, control: &OperationControl) -> Result<PathBuf> {
    let mut scope = Scope {
        control,
        supplemental: || Ok(()),
    };
    let result = find(start, &mut scope);
    scope.finish(result, [])
}
fn regular(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    metadata.is_file() && !metadata.file_type().is_symlink()
}
fn read_file<T, C: FnMut() -> Result<()>>(
    directory: &storage::Directory,
    path: &Path,
    name: &str,
    scope: &mut Scope<'_, C>,
    decode: impl FnOnce(&[u8], &mut Scope<'_, C>) -> Result<T>,
) -> Result<Option<T>> {
    scope.check()?;
    let metadata = match fs::symlink_metadata(path.join(name)) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            scope.check()?;
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    if !regular(&metadata) || metadata.len() > 65536 {
        return Err(format());
    }
    scope.check()?;
    let Some(mut file) =
        directory.open_optional_project_file_controlled(OsStr::new(name), scope)?
    else {
        scope.check()?;
        return Ok(None);
    };
    let result = (|| {
        scope.check()?;
        let metadata = file.metadata()?;
        if !regular(&metadata) {
            return Err(format());
        }
        let mut raw = Vec::with_capacity(65537);
        let mut buffer = [0; 8192];
        while raw.len() < 65537 {
            scope.check()?;
            let requested = buffer.len().min(65537 - raw.len());
            let n = file.read(&mut buffer[..requested])?;
            raw.extend_from_slice(&buffer[..n]);
            scope.check()?;
            if n == 0 {
                break;
            }
        }
        if raw.len() > 65536 || std::str::from_utf8(&raw).is_err() {
            return Err(format());
        }
        wire::validate(&raw, scope)?;
        decode(&raw, scope).map(Some)
    })();
    let cleanup = storage::close_file(file).err().map(ToolkitError::from);
    scope.finish(result, cleanup)
}
fn inspect<C: FnMut() -> Result<()>>(path: &Path, scope: &mut Scope<'_, C>) -> Result<PinnedSet> {
    management::inspect(path, scope)?
        .lock
        .map(|lock| lock.selection)
        .ok_or_else(|| error("selection.project_unlocked"))
}

pub fn resolve_pinned_controlled_with_check(
    inv: &Invocation,
    supplemental: impl FnMut() -> Result<()>,
) -> Result<PinnedSet> {
    let mut scope = Scope {
        control: &inv.control,
        supplemental,
    };
    let result = (|| {
        scope.check()?;
        if let Some(id) = inv.flag("--snapshot").filter(|id| !id.is_empty()) {
            return load_pinned_controlled_with_check(&inv.home, id, &inv.control, || {
                (scope.supplemental)()
            });
        }
        let path = match inv.flag("--project").filter(|path| !path.is_empty()) {
            Some(path) => absolute(&inv.cwd.join(path))?,
            None => find(&inv.cwd, &mut scope)?,
        };
        let pin = inspect(&path, &mut scope)?;
        // Go LoadProjectSelection creates only the immutable pin document, then
        // authenticates the actual persisted winner even on generation conflict.
        let mut metadata =
            Metadata::open_controlled_with_check(&inv.home, true, &inv.control, || {
                (scope.supplemental)()
            })?;
        let result = (|| {
            let key = format!("pin/{}", pin.id);
            match metadata
                .create_controlled_with_check(&key, &pin, &inv.control, || (scope.supplemental)())
            {
                Ok(_) => {}
                Err(error) if pure(&error, "vault.generation_conflict") => {}
                Err(error) => return Err(error),
            }
            let raw = metadata
                .read_bytes_controlled_with_check(&key, &inv.control, || (scope.supplemental)())?;
            let winner = super::file::stored_pin(&raw, &pin.id, &inv.control)?;
            scope.check()?;
            Ok(winner)
        })();
        let cleanup = metadata.close().err();
        scope.finish(result, cleanup)
    })();
    scope.finish(result, [])
}

#[cfg(test)]
mod tests;
