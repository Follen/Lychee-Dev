//! Durable archive selection for the existing Delivery slot-reset transaction.
//! This reference is recovery metadata, never connection or input authority.
use super::*;
use serde::{Deserialize, Serialize};

const SCHEMA: &str = "lycheedev.doctor-slot-reset.v1";
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ResetReference {
    schema: String,
    client: PathBuf,
    version: String,
    pub(super) archive: PathBuf,
}
fn conflict(detail: &str) -> ChannelError {
    lycheedev_core::ToolkitError::new("delivery.installation_conflict", detail, 4).into()
}
fn slot_archive(archive: &Path) -> PathBuf {
    let mut name = archive.as_os_str().to_owned();
    name.push(".slots");
    name.into()
}
pub(super) fn base_archive(archive: &Path) -> String {
    let name = archive.to_string_lossy();
    name.strip_suffix(".slots").unwrap_or(&name).to_owned()
}
// Match Windows' ordinary absolute spelling used by the persisted contract,
// including when Rust canonicalize returns a verbatim drive/UNC prefix.
fn client_path(path: PathBuf) -> PathBuf {
    let name = path.to_string_lossy();
    if let Some(unc) = name.strip_prefix("\\\\?\\UNC\\") {
        return PathBuf::from(format!("\\\\{unc}"));
    }
    name.strip_prefix("\\\\?\\")
        .map_or(path.clone(), PathBuf::from)
}
pub(super) fn reference_path(project: &Path, client: &Path) -> ChannelResult<(PathBuf, PathBuf)> {
    let client = client_path(std::fs::canonicalize(client).map_err(durable_journal::io_error)?);
    let digest =
        lycheedev_core::storage::sha256(client.to_string_lossy().to_lowercase().as_bytes());
    Ok((
        project
            .join(".lycheedev/live/repairs")
            .join(format!("{digest}.pending-reset.json")),
        client,
    ))
}
pub(super) fn read_reference(
    path: &Path,
    client: &Path,
    version: &str,
    explicit: Option<&Path>,
    archive_result: &mut String,
    control: &OperationControl,
) -> ChannelResult<Option<ResetReference>> {
    let Some(bytes) =
        durable_journal::read_file(path, 16384, "live.channel_doctor_reset_invalid", control)?
    else {
        return Ok(None);
    };
    let reference: ResetReference =
        serde_json::from_slice(&bytes).map_err(|_| error("live.channel_doctor_reset_invalid"))?;
    *archive_result = base_archive(&reference.archive);
    if reference.schema != SCHEMA
        || reference.client.to_string_lossy().to_lowercase()
            != client.to_string_lossy().to_lowercase()
        || reference.version != version
        || !reference.archive.is_absolute()
        || !reference.archive.to_string_lossy().ends_with(".slots")
    {
        return Err(conflict(
            "pending doctor slot reset does not match this installation and version",
        ));
    }
    if let Some(explicit) = explicit {
        let explicit = std::path::absolute(explicit).map_err(durable_journal::io_error)?;
        if slot_archive(&explicit).to_string_lossy().to_lowercase()
            != reference.archive.to_string_lossy().to_lowercase()
        {
            return Err(conflict(
                "--output differs from the pending doctor slot reset archive",
            ));
        }
    }
    Ok(Some(reference))
}
pub(super) fn reset(
    project: &Path,
    target: &ConnectionTarget,
    version: &str,
    explicit: Option<&Path>,
    stamp: &str,
    archive_result: &mut String,
    delivery: &mut impl RepairDelivery,
    control: &OperationControl,
) -> ChannelResult<PathBuf> {
    let (path, client) = reference_path(project, Path::new(&target.client.directory))?;
    let directory = path
        .parent()
        .ok_or_else(|| error("live.channel_project_invalid"))?;
    check_control(control)?;
    std::fs::create_dir_all(directory).map_err(durable_journal::io_error)?;
    let directory = std::fs::canonicalize(directory).map_err(durable_journal::io_error)?;
    let locks = directory.join(".locks");
    std::fs::create_dir_all(&locks).map_err(durable_journal::io_error)?;
    let lease = durable_journal::Lease::acquire(
        &locks,
        &client.to_string_lossy().to_lowercase(),
        true,
        control,
    )?;
    let result = (|| {
        let reference =
            match read_reference(&path, &client, version, explicit, archive_result, control)? {
                Some(reference) => reference,
                None => {
                    let base = match explicit {
                        Some(path) => {
                            std::path::absolute(path).map_err(durable_journal::io_error)?
                        }
                        None => {
                            let path = default_repair_archive(project, &client, stamp);
                            std::fs::create_dir_all(
                                path.parent()
                                    .ok_or_else(|| error("live.channel_project_invalid"))?,
                            )
                            .map_err(durable_journal::io_error)?;
                            path
                        }
                    };
                    let reference = ResetReference {
                        schema: SCHEMA.into(),
                        client,
                        version: version.into(),
                        archive: slot_archive(&base),
                    };
                    *archive_result = base_archive(&reference.archive);
                    let bytes = serde_json::to_vec(&reference)
                        .map_err(|_| error("live.channel_doctor_reset_invalid"))?;
                    lycheedev_core::storage::atomic_write_controlled(&path, &bytes, control)
                        .map_err(durable_journal::storage_error)?;
                    reference
                }
            };
        *archive_result = base_archive(&reference.archive);
        if !delivery.reset_slots(target, &reference.archive, version, control)? {
            return Err(super::super::super::driver::pending());
        }
        check_control(control)?;
        std::fs::remove_file(&path).map_err(durable_journal::io_error)?;
        Ok(PathBuf::from(base_archive(&reference.archive)))
    })();
    match (result, lease.close()) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(a), Err(b)) => Err(ChannelError::Joined { causes: vec![a, b] }),
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}
