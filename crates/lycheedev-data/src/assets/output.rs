//! Stable external destinations. Existing parents are resolved once, preserving
//! the frozen parent-alias contract; only the named output entry is replaceable.
use lycheedev_core::{
    Invocation, Result, ToolkitError,
    storage::{Destination, Directory},
};
use std::{
    fs, io,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::MetadataExt,
    },
    path::{Path, PathBuf},
};

pub fn ordinary(path: &Path) -> PathBuf {
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let prefix: Vec<u16> = "\\\\?\\".encode_utf16().collect();
    if wide.starts_with(&prefix) {
        let unc: Vec<u16> = "UNC\\".encode_utf16().collect();
        if wide[4..].starts_with(&unc) {
            let mut result = vec![92, 92];
            result.extend_from_slice(&wide[8..]);
            return PathBuf::from(std::ffi::OsString::from_wide(&result));
        }
        return PathBuf::from(std::ffi::OsString::from_wide(&wide[4..]));
    }
    path.to_owned()
}
fn path_error(message: impl Into<String>) -> ToolkitError {
    ToolkitError::new(
        "records.export_path",
        format!("records.export_path: {}", message.into()),
        2,
    )
}
fn conflict(path: &Path) -> ToolkitError {
    ToolkitError::new(
        "records.export_conflict",
        format!("records.export_conflict: {}", path.display()),
        3,
    )
}
fn outside(home: &Path, parent: &Path) -> Result<()> {
    let home = ordinary(&fs::canonicalize(home)?);
    let parent = ordinary(&fs::canonicalize(parent)?);
    // Windows component names compare case-insensitively, including caller
    // spelling. Both operands are resolved physical names before comparison.
    let home_components: Vec<_> = home
        .components()
        .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    let parent_components: Vec<_> = parent
        .components()
        .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    if parent_components.starts_with(&home_components) {
        return Err(path_error("output must be outside the managed workspace"));
    }
    Ok(())
}
pub fn publication_error(error: io::Error, path: &Path) -> ToolkitError {
    if let Some(control) = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<ToolkitError>())
    {
        return control.clone();
    }
    if error.kind() == io::ErrorKind::AlreadyExists {
        conflict(path)
    } else if error.kind() == io::ErrorKind::InvalidInput {
        path_error(error.to_string())
    } else {
        ToolkitError::io(error)
    }
}
pub fn destination(home: &Path, output: &Path, overwrite: bool) -> Result<Destination> {
    let destination = open_destination(output, overwrite)?;
    outside(
        home,
        destination
            .path()
            .parent()
            .expect("validated output parent"),
    )?;
    Ok(destination)
}
/// SQL CSV must check both existing entries before rejecting a managed-workspace
/// target. Keep both original parent-handle chains until pair publication ends.
pub(crate) fn destination_pair(
    home: &Path,
    data: &Path,
    manifest: &Path,
    overwrite: bool,
) -> Result<(Destination, Destination)> {
    let data = open_destination(data, overwrite)?;
    let manifest = open_destination(manifest, overwrite)?;
    outside(home, data.path().parent().expect("validated output parent"))?;
    outside(
        home,
        manifest.path().parent().expect("validated output parent"),
    )?;
    Ok((data, manifest))
}
fn open_destination(output: &Path, overwrite: bool) -> Result<Destination> {
    if output.as_os_str().is_empty()
        || matches!(output.as_os_str().encode_wide().last(), Some(47 | 92))
    {
        return Err(ToolkitError::new(
            "records.export_path",
            "records.export_path",
            2,
        ));
    }
    let name = output
        .file_name()
        .ok_or_else(|| path_error("output must name a file"))?;
    let absolute = std::path::absolute(output).map_err(|error| path_error(error.to_string()))?;
    let parent = absolute
        .parent()
        .ok_or_else(|| path_error("output must name a file"))?;
    let parent =
        ordinary(&fs::canonicalize(parent).map_err(|error| path_error(error.to_string()))?);
    let path = parent.join(name);
    // Conflict preflight deliberately precedes source access and the workspace
    // exclusion check, matching the frozen external-file contract.
    let destination = Destination::open(&path, overwrite).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            conflict(&path)
        } else {
            path_error(error.to_string())
        }
    })?;
    Ok(destination)
}
pub fn publish(
    inv: &Invocation,
    destination: &Destination,
    raw: &[u8],
    maximum: usize,
) -> Result<()> {
    destination
        .publish(raw, maximum as u64, || {
            inv.control
                .check()
                .map_err(|error| io::Error::new(io::ErrorKind::Interrupted, error))
        })
        .map_err(|error| publication_error(error, destination.path()))
}
pub fn publish_reader(
    inv: &Invocation,
    destination: &Destination,
    reader: &mut impl io::Read,
    length: u64,
    maximum: usize,
) -> Result<()> {
    destination
        .publish_reader(reader, length, maximum as u64, || {
            inv.control
                .check()
                .map_err(|error| io::Error::new(io::ErrorKind::Interrupted, error))
        })
        .map_err(|error| publication_error(error, destination.path()))
}
pub struct OutputDirectory {
    pub path: PathBuf,
    held: Directory,
    overwrite: bool,
}
impl OutputDirectory {
    pub fn open(home: &Path, output: &Path, overwrite: bool) -> Result<Self> {
        let absolute =
            std::path::absolute(output).map_err(|error| path_error(error.to_string()))?;
        if !fs::metadata(&absolute).is_ok_and(|metadata| metadata.is_dir()) {
            return Err(path_error(format!(
                "{} must be an existing directory",
                output.display()
            )));
        }
        let canonical =
            ordinary(&fs::canonicalize(absolute).map_err(|error| path_error(error.to_string()))?);
        outside(home, &canonical)?;
        let held = Directory::open(&canonical).map_err(|error| path_error(error.to_string()))?;
        Ok(Self {
            path: canonical,
            held,
            overwrite,
        })
    }
    pub fn preflight(&self, name: &str) -> Result<()> {
        match self.held.metadata(std::ffi::OsStr::new(name)) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(path_error(error.to_string())),
            Ok(metadata)
                if metadata.is_file()
                    && metadata.file_attributes() & 0x400 == 0
                    && self.overwrite =>
            {
                Ok(())
            }
            Ok(_) => Err(conflict(&self.path.join(name))),
        }
    }
    pub fn publish(&self, inv: &Invocation, name: &str, raw: &[u8], maximum: usize) -> Result<()> {
        self.preflight(name)?;
        let destination = Destination::open(&self.path.join(name), self.overwrite)
            .map_err(|error| publication_error(error, &self.path.join(name)))?;
        publish(inv, &destination, raw, maximum)
    }
}
