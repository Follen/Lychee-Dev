//! Read-only Windows installation/signature metadata for required vendor SDKs.
use lycheedev_core::{Result, ToolkitError, control::OperationControl, process};
use serde::{Deserialize, Serialize};
use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
};
#[derive(Serialize)]
pub(crate) struct Lookup<'a> {
    pub(crate) name_prefix: &'a str,
    pub(crate) program_subdirectory: Option<&'a str>,
    pub(crate) executable: &'a str,
    pub(crate) resource: &'a str,
    pub(crate) publishers: &'a [&'a str],
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Installation {
    pub(crate) directory: PathBuf,
    pub(crate) executable: PathBuf,
    pub(crate) resource: PathBuf,
    pub(crate) publisher: String,
    pub(crate) version: String,
}
pub(crate) fn lookup(request: &Lookup<'_>, control: &OperationControl) -> Result<Installation> {
    lookup_script(request, include_str!("desktop_runtime.ps1"), control)
}
fn lookup_script(
    request: &Lookup<'_>,
    script: &str,
    control: &OperationControl,
) -> Result<Installation> {
    let value = execute(request, script, control)?;
    let installation: Installation = serde_json::from_value(value).map_err(|_| missing())?;
    if !request
        .publishers
        .iter()
        .any(|value| value.eq_ignore_ascii_case(&installation.publisher))
        || !installation.executable.is_file()
        || !installation.resource.is_file()
    {
        return Err(missing());
    }
    lycheedev_core::storage::guard_path_with_check(
        &installation.directory,
        &installation.executable,
        || control.check(),
    )?;
    lycheedev_core::storage::guard_path_with_check(
        &installation.directory,
        &installation.resource,
        || control.check(),
    )?;
    Ok(installation)
}
fn execute(
    request: &impl Serialize,
    script: &str,
    control: &OperationControl,
) -> Result<serde_json::Value> {
    let system = lycheedev_windows::filesystem::system_directory()?;
    let shell = system.join("WindowsPowerShell/v1.0/powershell.exe");
    let mut command = Command::new(shell);
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .env(
            "PSModulePath",
            system.join("WindowsPowerShell/v1.0/Modules"),
        )
        .creation_flags(0x08000000);
    let result = process::run(
        command,
        Some(serde_json::to_vec(request)?),
        32768,
        32768,
        control,
    )?;
    if result.status != 0 {
        return Err(missing());
    }
    crate::input::value(&result.stdout).map_err(|_| missing())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VerifiedImage {
    pub(crate) path: PathBuf,
    pub(crate) publisher: String,
    pub(crate) version: String,
}
pub(crate) fn verify_image(
    path: &Path,
    publishers: &[&str],
    control: &OperationControl,
) -> Result<VerifiedImage> {
    if !path.is_absolute() {
        return Err(missing());
    }
    let value = execute(
        &serde_json::json!({"path":path,"publishers":publishers}),
        include_str!("desktop_signature.ps1"),
        control,
    )?;
    let image: VerifiedImage = serde_json::from_value(value).map_err(|_| missing())?;
    if !publishers
        .iter()
        .any(|p| p.eq_ignore_ascii_case(&image.publisher))
        || image.path.canonicalize()? != path.canonicalize()?
    {
        return Err(missing());
    }
    Ok(image)
}

#[cfg(test)]
mod tests;
fn missing() -> ToolkitError {
    ToolkitError::new(
        "publish.desktop_sdk",
        "a publisher-verified official desktop installation with the required SDK resource was not found",
        3,
    )
}
