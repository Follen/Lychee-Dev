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

/// Preserve every real reverse-order close event, then the original stop.
pub(crate) fn finish_closes<T>(
    mut result: Result<T>,
    closes: std::result::Result<(), Vec<io::Error>>,
    control: &lycheedev_core::control::OperationControl,
) -> Result<T> {
    if let Err(errors) = closes {
        for error in errors {
            result = lycheedev_core::metadata::historical_write::finish_historical(
                result,
                Err(error.into()),
            );
        }
    }
    match control.check() {
        Ok(()) => result,
        Err(stop)
            if result.as_ref().err().is_some_and(|error| {
                error.code == stop.code
                    && error.exit_code == stop.exit_code
                    && error.message == stop.message
                    && error.details == stop.details
            }) =>
        {
            result
        }
        Err(stop) => {
            lycheedev_core::metadata::historical_write::finish_historical(result, Err(stop))
        }
    }
}

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
pub(crate) fn destination_scoped(
    inv: &Invocation,
    output: &Path,
    overwrite: bool,
) -> Result<Destination> {
    let destination = open_destination_scoped(inv, output, overwrite, None)?;
    let result = (|| {
        inv.control.check()?;
        outside(
            &inv.home,
            destination
                .path()
                .parent()
                .expect("validated output parent"),
        )?;
        inv.control.check()
    })();
    match result {
        Ok(()) => Ok(destination),
        Err(error) => finish_closes(Err(error), destination.close(), &inv.control),
    }
}
fn open_destination_scoped(
    inv: &Invocation,
    output: &Path,
    overwrite: bool,
    budget: Option<&crate::resource::Budget>,
) -> Result<Destination> {
    use lycheedev_core::storage::FileControl;
    let mut control = PublicationControl { inv, budget };
    control.check().map_err(|e| publication_error(e, output))?;
    if output.as_os_str().is_empty()
        || matches!(output.as_os_str().encode_wide().last(), Some(47 | 92))
    {
        return Err(path_error("output must name a file"));
    }
    let name = output
        .file_name()
        .ok_or_else(|| path_error("output must name a file"))?;
    let absolute = std::path::absolute(output).map_err(|e| path_error(e.to_string()))?;
    let parent = absolute
        .parent()
        .ok_or_else(|| path_error("output must name a file"))?;
    let parent = ordinary(&fs::canonicalize(parent).map_err(|e| path_error(e.to_string()))?);
    control.check().map_err(|e| publication_error(e, output))?;
    let path = parent.join(name);
    let destination = Destination::prepare_controlled(&path, overwrite, &mut control)
        .map_err(|e| publication_error(e, &path))?;
    let result = destination
        .check_controlled(&mut control)
        .map_err(|e| publication_error(e, &path));
    match result {
        Ok(()) => Ok(destination),
        Err(error) => finish_closes(Err(error), destination.close(), &inv.control),
    }
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
/// Same data/manifest preflight order as the legacy pair, with one checked
/// parent owner per destination and original invocation/budget at every step.
pub(crate) fn destination_pair_scoped(
    inv: &Invocation,
    data: &Path,
    manifest: &Path,
    overwrite: bool,
    budget: &crate::resource::Budget,
) -> Result<(Destination, Destination)> {
    let data = open_destination_scoped(inv, data, overwrite, Some(budget))?;
    let manifest = match open_destination_scoped(inv, manifest, overwrite, Some(budget)) {
        Ok(value) => value,
        Err(error) => {
            return pair_stop(
                finish_closes(Err(error), data.close(), &inv.control),
                budget,
            );
        }
    };
    let result = (|| {
        inv.control.check()?;
        budget.check()?;
        outside(&inv.home, data.path().parent().expect("validated parent"))?;
        inv.control.check()?;
        budget.check()?;
        outside(
            &inv.home,
            manifest.path().parent().expect("validated parent"),
        )?;
        inv.control.check()?;
        budget.check()
    })();
    match result {
        Ok(()) => Ok((data, manifest)),
        Err(error) => finish_pair(inv, data, manifest, budget, Err(error), false),
    }
}
pub(crate) fn check_destination_scoped(
    inv: &Invocation,
    destination: &Destination,
    budget: &crate::resource::Budget,
) -> Result<()> {
    destination
        .check_controlled(&mut PublicationControl {
            inv,
            budget: Some(budget),
        })
        .map_err(|error| publication_error(error, destination.path()))
}
fn pair_stop<T>(result: Result<T>, budget: &crate::resource::Budget) -> Result<T> {
    match budget.check() {
        Ok(()) => result,
        Err(stop)
            if result.as_ref().err().is_some_and(|e| {
                e.code == stop.code
                    && e.exit_code == stop.exit_code
                    && e.message == stop.message
                    && e.details == stop.details
            }) =>
        {
            result
        }
        Err(stop) => {
            lycheedev_core::metadata::historical_write::finish_historical(result, Err(stop))
        }
    }
}
/// A pair caller consumes both owners even if encoding/capture/publication fails.
/// Successful publishing followed by close failure retains both committed paths.
pub(crate) fn finish_pair<T>(
    inv: &Invocation,
    data: Destination,
    manifest: Destination,
    budget: &crate::resource::Budget,
    result: Result<T>,
    published: bool,
) -> Result<T> {
    let paths = if published {
        Some((data.path().to_owned(), manifest.path().to_owned()))
    } else {
        None
    };
    let mut errors = manifest.close().err().unwrap_or_default();
    if let Err(data_errors) = data.close() {
        errors.extend(data_errors);
    }
    let closes = if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    };
    let result = pair_stop(finish_closes(result, closes, &inv.control), budget);
    if let Some((data, manifest)) = paths {
        result.map_err(|error| {
            publication_context(
                publication_context(
                    error,
                    &data,
                    lycheedev_core::storage::StagedPublication::Published,
                ),
                &manifest,
                lycheedev_core::storage::StagedPublication::Published,
            )
        })
    } else {
        result
    }
}
/// Reuse the existing typed single-output facts; callers may nest two real
/// attempts without inventing an atomic pair transaction or replaying a stage.
pub(crate) fn publication_context(
    mut error: ToolkitError,
    path: &Path,
    publication: lycheedev_core::storage::StagedPublication,
) -> ToolkitError {
    use lycheedev_core::storage::StagedPublication;
    let primary = error.clone();
    error.details = serde_json::json!({"schema":"lycheedev.asset-publication-error.v1", "primary":primary,
        "publication":{"state":match publication {
            StagedPublication::NotPublished=>"not_published", StagedPublication::Published=>"published", StagedPublication::Unknown=>"unknown"
        },"path":path.to_string_lossy()}});
    if publication != StagedPublication::NotPublished {
        error
            .message
            .push_str(if publication == StagedPublication::Published {
                "\noutput already published: "
            } else {
                "\noutput publication unknown; do not replay: "
            });
        error.message.push_str(&path.to_string_lossy());
    }
    error
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
    pub(crate) fn open_scoped(inv: &Invocation, output: &Path, overwrite: bool) -> Result<Self> {
        inv.control.check()?;
        let absolute = std::path::absolute(output).map_err(|e| path_error(e.to_string()))?;
        if !fs::metadata(&absolute).is_ok_and(|metadata| metadata.is_dir()) {
            return Err(path_error(format!(
                "{} must be an existing directory",
                output.display()
            )));
        }
        inv.control.check()?;
        let canonical =
            ordinary(&fs::canonicalize(absolute).map_err(|e| path_error(e.to_string()))?);
        inv.control.check()?;
        outside(&inv.home, &canonical)?;
        inv.control.check()?;
        let held =
            Directory::open_controlled(&canonical, &mut PublicationControl { inv, budget: None })
                .map_err(|e| publication_error(e, &canonical))?;
        let owner = Self {
            path: canonical,
            held,
            overwrite,
        };
        match inv.control.check() {
            Ok(()) => Ok(owner),
            Err(error) => finish_closes(Err(error), owner.close(), &inv.control),
        }
    }
    pub(crate) fn preflight_scoped(
        &self,
        inv: &Invocation,
        name: &str,
        budget: Option<&crate::resource::Budget>,
    ) -> Result<()> {
        let found = self
            .held
            .optional_metadata_controlled(
                std::ffi::OsStr::new(name),
                &mut PublicationControl { inv, budget },
            )
            .map_err(|e| publication_error(e, &self.path.join(name)))?;
        match found {
            None => Ok(()),
            Some(metadata)
                if metadata.is_file()
                    && metadata.file_attributes() & 0x400 == 0
                    && self.overwrite =>
            {
                Ok(())
            }
            Some(_) => Err(conflict(&self.path.join(name))),
        }
    }
    pub fn close(self) -> std::result::Result<(), Vec<io::Error>> {
        self.held.close()
    }
    pub(crate) fn lease(
        &self,
        inv: &Invocation,
        budget: Option<&crate::resource::Budget>,
    ) -> Result<lycheedev_core::storage::ExclusiveLock> {
        let scope = self
            .path
            .parent()
            .ok_or_else(|| path_error("output requires parent"))?
            .join(".lycheedev-locks");
        let name = self
            .path
            .file_name()
            .ok_or_else(|| path_error("output requires name"))?
            .to_string_lossy();
        let resource = format!(
            "asset-demux:{}",
            name.chars()
                .map(|c| crate::unicode_simple::map(c, false))
                .collect::<String>()
        );
        lycheedev_core::storage::ExclusiveLock::resource_wait_controlled_with_check(
            &scope,
            &resource,
            &inv.control,
            || budget.map_or(Ok(()), crate::resource::Budget::check),
        )
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
        let result = publish(inv, &destination, raw, maximum);
        finish_closes(result, destination.close(), &inv.control)
    }
}

struct PublicationControl<'a> {
    inv: &'a Invocation,
    budget: Option<&'a crate::resource::Budget>,
}
impl lycheedev_core::storage::FileControl for PublicationControl<'_> {
    type Error = io::Error;
    fn check(&mut self) -> io::Result<()> {
        self.inv
            .control
            .check()
            .and_then(|()| self.budget.map_or(Ok(()), crate::resource::Budget::check))
            .map_err(|error| io::Error::new(io::ErrorKind::Interrupted, error))
    }
    fn charge(&mut self, cost: lycheedev_core::storage::FileCost) -> io::Result<()> {
        self.check()?;
        if let Some(budget) = self.budget {
            let bytes = usize::try_from(cost.allocation_bytes)
                .map_err(|_| io::Error::other("publication allocation overflow"))?;
            let work = usize::try_from(cost.work_units.saturating_add(cost.io_operations))
                .map_err(|_| io::Error::other("publication work overflow"))?;
            budget
                .safety_only()
                .charge(crate::resource::Cost::metadata(bytes))
                .and_then(|()| {
                    budget
                        .safety_only()
                        .charge(crate::resource::Cost::work(work))
                })
                .map_err(|error| io::Error::new(io::ErrorKind::Interrupted, error))?;
        }
        Ok(())
    }
}
/// One native stage attempt; observation cannot request or replay publication.
/// Facts are recorded even when the original stage close subsequently failed.
pub(crate) fn publish_reader_scoped(
    inv: &Invocation,
    destination: &Destination,
    reader: &mut impl io::Read,
    length: u64,
    maximum: usize,
    budget: Option<&crate::resource::Budget>,
    observe: &mut impl FnMut(lycheedev_core::storage::StagedPublication),
) -> Result<()> {
    use lycheedev_core::storage::StagedPublication;
    let attempt = destination.publish_reader_controlled(
        reader,
        length,
        maximum as u64,
        &mut PublicationControl { inv, budget },
    );
    observe(attempt.publication);
    let result = attempt
        .result
        .map_err(|error| publication_error(error, destination.path()));
    let result = finish_closes(
        result,
        if attempt.cleanup.is_empty() {
            Ok(())
        } else {
            Err(attempt.cleanup)
        },
        &inv.control,
    );
    let result = match budget.and_then(|b| b.check().err()) {
        None => result,
        Some(stop)
            if result.as_ref().err().is_some_and(|error| {
                error.code == stop.code
                    && error.exit_code == stop.exit_code
                    && error.message == stop.message
                    && error.details == stop.details
            }) =>
        {
            result
        }
        Some(stop) => {
            lycheedev_core::metadata::historical_write::finish_historical(result, Err(stop))
        }
    };
    result.map_err(|mut error| {
        let primary = error.clone();
        error.details = serde_json::json!({"schema":"lycheedev.asset-publication-error.v1", "primary":primary,
            "publication":{"state":match attempt.publication {
                StagedPublication::NotPublished=>"not_published", StagedPublication::Published=>"published", StagedPublication::Unknown=>"unknown"
            },"path":destination.path().to_string_lossy()}});
        if attempt.publication != StagedPublication::NotPublished {
            error.message.push_str(if attempt.publication == StagedPublication::Published { "\nasset output already published: " } else { "\nasset output publication unknown; do not replay: " });
            error.message.push_str(&destination.path().to_string_lossy());
        }
        error
    })
}
impl OutputDirectory {
    pub(crate) fn publish_scoped(
        &self,
        inv: &Invocation,
        name: &str,
        raw: &[u8],
        maximum: usize,
        budget: Option<&crate::resource::Budget>,
        observe: &mut impl FnMut(lycheedev_core::storage::StagedPublication),
    ) -> Result<()> {
        self.preflight_scoped(inv, name, budget)?;
        let destination =
            open_destination_scoped(inv, &self.path.join(name), self.overwrite, budget)?;
        let result = publish_reader_scoped(
            inv,
            &destination,
            &mut io::Cursor::new(raw),
            raw.len() as u64,
            maximum,
            budget,
            observe,
        );
        finish_closes(result, destination.close(), &inv.control)
    }
}

#[cfg(test)]
#[path = "output/scoped_tests.rs"]
mod scoped_tests;
