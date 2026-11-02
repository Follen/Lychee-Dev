//! Selected filesystem trees and ZIPs shared by author backup adapters.
//! Directory names describe locations; they never identify a platform addon.
use crate::input::invalid;
use lycheedev_core::{Result, ToolkitError, control::OperationControl, storage};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

pub(crate) fn md5(file: &mut File, control: &OperationControl) -> Result<String> {
    use md5::{Digest, Md5};
    file.seek(SeekFrom::Start(0))?;
    let mut hash = Md5::new();
    let mut bytes = [0u8; 65536];
    loop {
        control.check()?;
        let n = file.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        hash.update(&bytes[..n]);
    }
    file.seek(SeekFrom::Start(0))?;
    Ok(hex::encode(hash.finalize()))
}

pub(crate) fn name(value: &str) -> Result<&str> {
    let base = value.split('.').next().unwrap_or("").to_ascii_uppercase();
    let device = matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        base.strip_prefix(prefix).is_some_and(|tail| {
            matches!(
                tail,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    });
    if value.is_empty()
        || matches!(value, "." | "..")
        || value.ends_with([' ', '.'])
        || value
            .chars()
            .any(|c| c < ' ' || "/\\:\0<>\"|?*".contains(c))
        || device
    {
        return Err(invalid(
            "$.selection",
            "expected one ordinary Windows file or directory name",
        ));
    }
    Ok(value)
}
pub(crate) fn path(root: &Path, parts: &[&str], control: &OperationControl) -> Result<PathBuf> {
    let mut path = root.to_path_buf();
    for part in parts {
        path.push(name(part)?);
    }
    storage::guard_path_with_check(root, &path, || control.check())?;
    Ok(path)
}
pub(crate) fn filename(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            invalid(
                "$.selection",
                "backup filenames must be representable as UTF-8",
            )
        })
}
pub(crate) fn resolve_output(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut base = absolute.clone();
    let mut missing = Vec::new();
    while !base.exists() {
        let component = filename(&base)?;
        name(&component)?;
        missing.push(component);
        if !base.pop() {
            return Err(invalid("$.output_dir", "output parent does not exist"));
        }
    }
    storage::guard_path(&base, &base)?;
    let mut result = fs::canonicalize(base)?;
    for component in missing.into_iter().rev() {
        result.push(component);
    }
    Ok(result)
}
pub(crate) fn common_sources(
    root: &Path,
    account: &Path,
    control: &OperationControl,
) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    for path in directory_entries(root, account, control)? {
        if path.is_file() || filename(&path)?.eq_ignore_ascii_case("SavedVariables") {
            storage::guard_path_with_check(root, &path, || control.check())?;
            result.push(path);
        }
    }
    Ok(result)
}
pub(crate) fn entries(
    root: &Path,
    directory: &Path,
    control: &OperationControl,
) -> Result<Vec<PathBuf>> {
    let paths = directory_entries(root, directory, control)?;
    for path in &paths {
        storage::guard_path_with_check(root, path, || control.check())?;
    }
    Ok(paths)
}
fn directory_entries(
    root: &Path,
    directory: &Path,
    control: &OperationControl,
) -> Result<Vec<PathBuf>> {
    storage::guard_path_with_check(root, directory, || control.check())?;
    if !directory.exists() {
        return Ok(Vec::new());
    }
    if !directory.is_dir() {
        return Err(invalid("$.game_dir", "expected a backup source directory"));
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)? {
        control.check()?;
        let path = entry?.path();
        name(&filename(&path)?)?;
        paths.push(path);
    }
    paths.sort_by_key(|path| path.file_name().unwrap().to_string_lossy().to_lowercase());
    Ok(paths)
}
#[derive(Serialize)]
pub(crate) struct Inventory {
    pub(crate) unknown_plugins: Vec<String>,
    pub(crate) materials: Vec<String>,
    pub(crate) fonts: Vec<Font>,
    pub(crate) accounts: Vec<Account>,
    pub(crate) blocked: Vec<String>,
}
#[derive(Serialize)]
pub(crate) struct Font {
    pub(crate) name: String,
    pub(crate) size: u64,
}
#[derive(Serialize)]
pub(crate) struct Account {
    pub(crate) account: String,
    pub(crate) roles: Vec<Role>,
    pub(crate) has_common: bool,
}
#[derive(Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Role {
    pub(crate) server: String,
    pub(crate) name: String,
}
fn candidates(
    root: &Path,
    directory: &Path,
    blocked: &mut Vec<String>,
    control: &OperationControl,
) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for path in directory_entries(root, directory, control)? {
        let metadata = fs::symlink_metadata(&path)?;
        #[cfg(windows)]
        let reparse = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let reparse = false;
        if metadata.file_type().is_symlink() || reparse {
            blocked.push(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        } else {
            paths.push(path);
        }
    }
    Ok(paths)
}
pub(crate) fn scan(root: &Path, control: &OperationControl) -> Result<Inventory> {
    if !root.is_dir() {
        return Err(invalid("$.game_dir", "game directory does not exist"));
    }
    storage::guard_path_with_check(root, root, || control.check())?;
    let mut result = Inventory {
        unknown_plugins: Vec::new(),
        materials: Vec::new(),
        fonts: Vec::new(),
        accounts: Vec::new(),
        blocked: Vec::new(),
    };
    for item in candidates(
        root,
        &root.join("Interface/AddOns"),
        &mut result.blocked,
        control,
    )? {
        if item.is_dir() {
            result.unknown_plugins.push(filename(&item)?);
        }
    }
    for item in candidates(root, &root.join("Interface"), &mut result.blocked, control)? {
        let file = filename(&item)?;
        if file.eq_ignore_ascii_case("AddOns") {
            continue;
        }
        let extension = item
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if item.is_dir() && !candidates(root, &item, &mut result.blocked, control)?.is_empty()
            || item.is_file()
                && matches!(
                    extension.as_str(),
                    "blp" | "tga" | "dds" | "png" | "jpeg" | "bmp" | "tex"
                )
        {
            result.materials.push(file);
        }
    }
    for item in candidates(root, &root.join("Fonts"), &mut result.blocked, control)? {
        if item.is_file()
            && item
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("ttf"))
        {
            result.fonts.push(Font {
                name: filename(&item)?,
                size: item.metadata()?.len(),
            });
        }
    }
    for account in candidates(
        root,
        &root.join("WTF/Account"),
        &mut result.blocked,
        control,
    )? {
        if !account.is_dir() {
            continue;
        }
        let members = candidates(root, &account, &mut result.blocked, control)?;
        let mut roles = Vec::new();
        for server in members.iter().filter(|item| {
            item.is_dir()
                && !item
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("SavedVariables")
        }) {
            for role in candidates(root, server, &mut result.blocked, control)? {
                if !role.is_dir() {
                    continue;
                }
                let files = candidates(root, &role, &mut result.blocked, control)?;
                if files.iter().any(|file| {
                    ["AddOns.txt", "bindings-cache.wtf", "config-cache.wtf"]
                        .iter()
                        .any(|marker| {
                            file.file_name()
                                .unwrap()
                                .to_string_lossy()
                                .eq_ignore_ascii_case(marker)
                        })
                }) {
                    roles.push(Role {
                        server: filename(server)?,
                        name: filename(&role)?,
                    });
                }
            }
        }
        if !roles.is_empty() {
            result.accounts.push(Account {
                account: filename(&account)?,
                roles,
                has_common: members.iter().any(|item| {
                    item.is_file()
                        || item
                            .file_name()
                            .unwrap()
                            .to_string_lossy()
                            .eq_ignore_ascii_case("SavedVariables")
                }),
            });
        }
    }
    Ok(result)
}
pub(crate) fn distinct_names(values: &[String]) -> Result<()> {
    let mut names = BTreeSet::new();
    for value in values {
        name(value)?;
        if !names.insert(value.to_lowercase()) {
            return Err(invalid(
                "$.selection",
                "backup selection repeats a Windows name",
            ));
        }
    }
    Ok(())
}
pub(crate) fn open_source(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    Ok(options.open(path)?)
}
pub(crate) fn copy(
    root: &Path,
    source: &Path,
    destination: &Path,
    control: &OperationControl,
) -> Result<()> {
    storage::guard_path_with_check(root, source, || control.check())?;
    let mut input = open_source(source)?;
    let result = (|| {
        let output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        let result = (|| {
            let mut output = &output;
            let mut buffer = [0u8; 65536];
            loop {
                control.check()?;
                let count = input.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                output.write_all(&buffer[..count])?;
            }
            output.sync_all()?;
            Ok(())
        })();
        storage::finish_file(output, result, control)
    })();
    storage::finish_file(input, result, control)
}
#[derive(Serialize)]
pub(crate) struct Artifact {
    pub(crate) file: String,
    pub(crate) size: u64,
    pub(crate) sha256: String,
    pub(crate) files: u64,
}
pub(crate) fn archive(
    root: &Path,
    sources: &[PathBuf],
    base: Option<&Path>,
    destination: &Path,
    exclude_bak: bool,
    control: &OperationControl,
) -> Result<Artifact> {
    let mut selected = Vec::new();
    for source in sources {
        storage::guard_path_with_check(root, source, || control.check())?;
        let before = selected.len();
        let mut pending = vec![source.clone()];
        while let Some(item) = pending.pop() {
            control.check()?;
            storage::guard_path_with_check(root, &item, || control.check())?;
            if item.is_dir() {
                pending.extend(entries(root, &item, control)?.into_iter().rev());
                continue;
            }
            if !item.is_file() {
                return Err(invalid(
                    "$.selection",
                    "backup member is not an ordinary file",
                ));
            }
            if exclude_bak && filename(&item)?.to_ascii_lowercase().ends_with(".bak") {
                continue;
            }
            let member = member_path(&item, base.unwrap_or_else(|| source.parent().unwrap()))?;
            selected.push((item, member));
        }
        if selected.len() == before && source.is_dir() {
            selected.push((
                source.clone(),
                format!(
                    "{}/",
                    member_path(source, base.unwrap_or_else(|| source.parent().unwrap()))?
                ),
            ));
        }
    }
    archive_entries(root, &selected, destination, control)
}
pub(crate) fn member_path(path: &Path, base: &Path) -> Result<String> {
    path.strip_prefix(base)
        .map_err(|_| invalid("$.selection", "backup file escaped its selected component"))?
        .components()
        .map(|part| {
            name(
                part.as_os_str()
                    .to_str()
                    .ok_or_else(|| invalid("$.selection", "backup member name is not UTF-8"))?,
            )
            .map(str::to_owned)
        })
        .collect::<Result<Vec<_>>>()
        .map(|parts| parts.join("/"))
}
/// Write the already selected members; callers with role-based selection must
/// not walk the containing account a second time while building its archive.
pub(crate) fn archive_entries(
    root: &Path,
    selected: &[(PathBuf, String)],
    destination: &Path,
    control: &OperationControl,
) -> Result<Artifact> {
    let mut output = OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .open(destination)?;
    let result = (|| {
        let mut archive = zip::ZipWriter::new(&mut output);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let mut count = 0;
        let mut seen = BTreeSet::new();
        for (item, member) in selected {
            control.check()?;
            storage::guard_path_with_check(root, &item, || control.check())?;
            let member_name = member.strip_suffix('/').unwrap_or(member);
            for part in member_name.split('/') {
                name(part)?;
            }
            if !seen.insert(member_name.to_lowercase()) {
                return Err(invalid(
                    "$.selection",
                    "backup ZIP repeats a Windows member path",
                ));
            }
            if member.ends_with('/') && item.is_dir() {
                archive.add_directory(member, options).map_err(zip_error)?;
                continue;
            }
            if !item.is_file() || member.ends_with('/') {
                return Err(invalid(
                    "$.selection",
                    "backup member is not an ordinary file",
                ));
            }
            archive.start_file(member, options).map_err(zip_error)?;
            let mut input = open_source(&item)?;
            let result = (|| {
                let mut buffer = [0u8; 65536];
                loop {
                    control.check()?;
                    let bytes = input.read(&mut buffer)?;
                    if bytes == 0 {
                        break;
                    }
                    archive.write_all(&buffer[..bytes])?;
                }
                Ok(())
            })();
            storage::finish_file(input, result, control)?;
            count += 1;
        }
        archive.finish().map_err(zip_error)?;
        output.sync_all()?;
        Ok(count)
    })();
    let count = storage::finish_file(output, result, control)?;
    let (file, sha256) = storage::locked_sha256(destination, u64::MAX, control)?;
    let size = file.metadata()?.len();
    storage::finish_file(file, Ok(()), control)?;
    Ok(Artifact {
        file: filename(destination)?,
        size,
        sha256,
        files: count,
    })
}
fn zip_error(error: zip::result::ZipError) -> ToolkitError {
    match error {
        zip::result::ZipError::Io(error) => ToolkitError::io(error),
        _ => invalid("$.output_dir", "backup ZIP could not be written"),
    }
}
