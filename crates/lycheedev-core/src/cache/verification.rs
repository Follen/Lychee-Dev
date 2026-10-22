//! Read-only Go cache verification. Body hashes use fixed-size streaming reads;
//! every acquired File is consumed by checked close under the original scope.
use super::*;
use crate::control::OperationControl;
use serde::Serialize;
use std::{ffi::OsStr, io::Read};
mod wire;
#[derive(Debug, Default, Serialize)]
pub(crate) struct Report {
    pub ok: bool,
    pub checked: usize,
    pub bytes: u64,
    pub missing: Vec<String>,
    pub corrupt: Vec<String>,
    pub errors: Vec<String>,
}
/// Go WalkDir does not follow directory symlinks and ignores actual NotFound.
/// Iteration checks the unchanged caller scope, without an artificial object cap.
pub(crate) fn walk_files(
    root: &Path,
    control: &OperationControl,
    mut visit: impl FnMut(&Path, std::io::Result<fs::Metadata>) -> Result<()>,
) -> Result<()> {
    enum Entry {
        Root(PathBuf),
        Child(fs::DirEntry),
    }
    let mut pending = vec![Entry::Root(root.to_owned())];
    while let Some(entry) = pending.pop() {
        control.check()?;
        let (path, directory, root_info) = match &entry {
            Entry::Root(path) => {
                let info = match fs::symlink_metadata(path) {
                    Ok(info) => info,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e.into()),
                };
                (path.clone(), info.is_dir(), Some(info))
            }
            Entry::Child(entry) => (entry.path(), entry.file_type()?.is_dir(), None),
        };
        control.check()?;
        if !directory {
            let info = match entry {
                Entry::Root(_) => Ok(root_info.expect("root metadata")),
                Entry::Child(entry) => entry.metadata(),
            };
            // Go obtains DirEntry.Info inside the visitor. A disappearing file
            // is an individual verification error and does not abort siblings.
            visit(&path, info)?;
            continue;
        }
        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        let mut children = Vec::new();
        for entry in entries {
            control.check()?;
            children.push(entry?);
        }
        control.check()?;
        children.sort_by_key(fs::DirEntry::file_name);
        pending.extend(children.into_iter().rev().map(Entry::Child));
    }
    control.check()
}

fn digest(root: &Path, path: &Path) -> Option<String> {
    let mut components = path.strip_prefix(root).ok()?.components();
    let first = components.next()?.as_os_str().to_str()?;
    let second = components.next()?.as_os_str().to_str()?;
    if components.next().is_some() || first.len() != 2 || second.len() != 62 {
        return None;
    }
    let value = format!("{first}{second}");
    valid_digest(&value, 64).then_some(value)
}
fn verify_file(
    home: &Path,
    path: &Path,
    digest: &str,
    length: u64,
    control: &OperationControl,
) -> Result<()> {
    storage::guard_path_with_check(home, path, || control.check())?;
    let (file, actual) = storage::locked_sha256(path, length, control)?;
    let result = (|| {
        if file.metadata()?.len() != length || actual != digest {
            return Err(ToolkitError::new(
                "vault.cache_integrity",
                "vault.cache_integrity",
                4,
            ));
        }
        control.check()
    })();
    storage::finish_file(file, result, control)
}
fn retain(home: &Path, digest: &str, control: &OperationControl) -> Result<bool> {
    let path = home.join("cache/retain").join(format!("{digest}.json"));
    control.check()?;
    let mut file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            control.check()?;
            return Ok(false);
        }
        Err(e) => return Err(e.into()),
    };
    let result = (|| {
        storage::guard_path_with_check(home, &path, || control.check())?;
        let mut raw = Vec::with_capacity(65537);
        let mut buffer = [0; 8192];
        while raw.len() < 65537 {
            control.check()?;
            let request = buffer.len().min(65537 - raw.len());
            let n = file.read(&mut buffer[..request])?;
            raw.extend_from_slice(&buffer[..n]);
            control.check()?;
            if n == 0 {
                break;
            }
        }
        if raw.len() > 65536 {
            return Err(wire::error(format!("corrupt retain marker for {digest}")));
        }
        let value: wire::Retain = serde_json::from_slice(&raw).map_err(wire::error)?;
        if value.sha256 != digest {
            return Err(wire::error("retain marker identity"));
        }
        for owner in value.owners.iter() {
            control.check()?;
            owner.validate()?;
        }
        control.check()?;
        Ok(!value.owners.is_empty())
    })();
    storage::finish_file(file, result, control)
}
fn stopped<T>(result: Result<T>, control: &OperationControl) -> Result<T> {
    crate::selection::selection_finish(result, control.check())
}
pub(crate) fn verify(home: &Path, control: &OperationControl) -> Result<Report> {
    control.check()?;
    let mut report = Report::default();
    let root = home.join("cache/objects");
    let result = walk_files(&root, control, |path, info| {
        let Some(digest) = digest(&root, path) else {
            report.errors.push(format!(
                "unexpected cache file: {}",
                path.file_name().unwrap_or(OsStr::new("")).to_string_lossy()
            ));
            return Ok(());
        };
        let info = match info {
            Ok(info) => info,
            Err(e) => {
                report.errors.push(e.to_string());
                return Ok(());
            }
        };
        report.checked = report
            .checked
            .checked_add(1)
            .ok_or_else(|| ToolkitError::invalid("cache object count overflow"))?;
        report.bytes = report
            .bytes
            .checked_add(info.len())
            .ok_or_else(|| ToolkitError::invalid("cache byte count overflow"))?;
        if let Err(error) = verify_file(home, path, &digest, info.len(), control) {
            // Verification failure remains a corrupt object, but a real stop
            // ends the request and retains the first typed cause and close facts.
            if control.check().is_err() {
                return stopped(Err(error), control);
            }
            report.corrupt.push(digest);
        }
        Ok(())
    });
    if let Err(error) = result {
        if control.check().is_err() {
            return Err(partial(error, report));
        }
        report.errors.push(error.message);
    }
    let result = walk_files(&home.join("cache/retain"), control, |path, _| {
        let name = path.file_name().unwrap_or(OsStr::new("")).to_string_lossy();
        let Some(digest) = name.strip_suffix(".json") else {
            return Ok(());
        };
        match retain(home, digest, control) {
            Ok(false) => {}
            Ok(true) => {
                // Validate only at the object-consumption boundary. Go accepts
                // otherwise unused retention references with an empty owner list.
                // Keep the established managed-path safety policy for references
                // that would be used to inspect a named object.
                if !valid_digest(digest, 64) {
                    report
                        .errors
                        .push(wire::error("retain marker identity").message);
                    return Ok(());
                }
                control.check()?;
                match fs::symlink_metadata(
                    home.join("cache/objects")
                        .join(&digest[..2])
                        .join(&digest[2..]),
                ) {
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        report.missing.push(digest.into())
                    }
                    Err(e) => report.errors.push(e.to_string()),
                }
                control.check()?;
            }
            Err(error) => {
                if control.check().is_err() {
                    return stopped(Err(error), control);
                }
                report.errors.push(error.message);
            }
        }
        Ok(())
    });
    if let Err(error) = result {
        if control.check().is_err() {
            return Err(partial(error, report));
        }
        report.errors.push(error.message);
    }
    report.missing.sort();
    report.corrupt.sort();
    report.errors.sort();
    report.ok = report.missing.is_empty() && report.corrupt.is_empty() && report.errors.is_empty();
    match control.check() {
        Ok(()) => Ok(report),
        Err(error) => Err(partial(error, report)),
    }
}
fn partial(mut error: ToolkitError, report: Report) -> ToolkitError {
    let primary = error.clone();
    error.details =
        json!({"schema":"lycheedev.cache-verification-error.v1","primary":primary,"result":report});
    error
}
#[cfg(test)]
mod tests;
