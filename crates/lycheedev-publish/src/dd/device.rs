//! One stable uploader device identity. Never write DD's QSettings/registry or
//! persist hardware serials/MACs; only the original MD5-derived clientNo is kept.
use super::cc::Device;
use lycheedev_core::{Result, ToolkitError, control::OperationControl, process, storage};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    os::windows::process::CommandExt,
    path::Path,
    process::Command,
};

const SCHEMA: &str = "lycheedev.publish.dd.device.v1";
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    schema: String,
    client_no: String,
}
pub(super) struct Facts {
    pub(super) node: [u8; 6],
    pub(super) version: [u32; 3],
    pub(super) serials: Option<Vec<String>>,
}
pub(super) fn load(home: &Path, control: &OperationControl) -> Result<Device> {
    load_with(home, control, |needs_serials| {
        control.check()?;
        let host = lycheedev_windows::host_metadata::current()
            .map_err(|_| invalid("Windows device metadata is unavailable"))?;
        let serials = if needs_serials {
            serials(control)?
        } else {
            None
        };
        Ok(Facts {
            node: host.uuid_node,
            version: host.windows_version,
            serials,
        })
    })
}
fn load_with(
    home: &Path,
    control: &OperationControl,
    facts: impl FnOnce(bool) -> Result<Facts>,
) -> Result<Device> {
    control.check()?;
    let directory = home.join("publish/dd");
    let path = directory.join("device.json");
    storage::guard_path_with_check(home, &path, || control.check())?;
    fs::create_dir_all(&directory)?;
    let lock = storage::ExclusiveLock::resource_controlled(&directory, "device", control)?;
    let result = (|| {
        let saved = match fs::symlink_metadata(&path) {
            Ok(_) => {
                let (mut file, _) = storage::locked_sha256(&path, 4096, control)?;
                let parsed = (|| {
                    file.seek(SeekFrom::Start(0))?;
                    let mut bytes = Vec::new();
                    file.read_to_end(&mut bytes)?;
                    let value = crate::input::value(&bytes)
                        .map_err(|_| invalid("saved DD device state is invalid"))?;
                    let saved: Saved = serde_json::from_value(value)
                        .map_err(|_| invalid("saved DD device state is invalid"))?;
                    if saved.schema != SCHEMA
                        || saved.client_no.len() != 32
                        || !saved
                            .client_no
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    {
                        return Err(invalid("saved DD device identity is invalid"));
                    }
                    Ok(saved.client_no)
                })();
                Some(storage::finish_file(file, parsed, control)?)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err(invalid("saved DD device state is unreadable")),
        };
        let facts = facts(saved.is_none())?;
        control.check()?;
        let client_no = if let Some(client_no) = saved {
            client_no
        } else {
            let source = facts
                .serials
                .as_ref()
                .map(|values| {
                    values
                        .iter()
                        .map(|s| s.replace(' ', "\r").trim().to_owned())
                        .collect::<Vec<_>>()
                        .join("-")
                        .trim_matches('-')
                        .to_owned()
                })
                .filter(|s| !s.is_empty());
            let source = match source {
                Some(value) => value,
                None => {
                    let mut bytes = lycheedev_windows::random::bytes::<16>()
                        .map_err(|_| invalid("DD device identity could not be generated"))?;
                    bytes[6] = (bytes[6] & 15) | 64;
                    bytes[8] = (bytes[8] & 63) | 128;
                    hex::encode(bytes)
                }
            };
            let client_no = format!("{:x}", Md5::digest(source.as_bytes()));
            storage::atomic_write_controlled(
                &path,
                &serde_json::to_vec(&Saved {
                    schema: SCHEMA.into(),
                    client_no: client_no.clone(),
                })?,
                control,
            )?;
            client_no
        };
        let mac = facts
            .node
            .iter()
            .map(|n| format!("{n:02x}"))
            .collect::<Vec<_>>()
            .join("-");
        let [major, minor, build] = facts.version;
        Ok(Device {
            client_no,
            mac,
            os_version: format!("{major}.{minor}.{build}"),
            bi_desc: json!({}),
        })
    })();
    let close = lock.close();
    match (result, close) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (_, Err(error)) => Err(error),
    }
}
fn serials(control: &OperationControl) -> Result<Option<Vec<String>>> {
    let system = lycheedev_windows::filesystem::system_directory()?;
    let mut command = Command::new(system.join("WindowsPowerShell/v1.0/powershell.exe"));
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            include_str!("device_serials.ps1"),
        ])
        .env(
            "PSModulePath",
            system.join("WindowsPowerShell/v1.0/Modules"),
        )
        .creation_flags(0x08000000);
    let output = process::run(command, None, 32768, 4096, control)?;
    if output.status != 0 {
        return Err(invalid("Windows hardware metadata query failed"));
    }
    let value = crate::input::value(&output.stdout)
        .map_err(|_| invalid("Windows hardware metadata is invalid"))?;
    match value {
        Value::Null => Ok(None),
        Value::Array(values) => values
            .into_iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| invalid("Windows hardware metadata is invalid"))
            })
            .collect::<Result<Vec<_>>>()
            .map(Some),
        _ => Err(invalid("Windows hardware metadata is invalid")),
    }
}
fn invalid(message: &str) -> ToolkitError {
    ToolkitError::new("publish.dd_device", message, 3)
}

#[cfg(test)]
mod tests;
