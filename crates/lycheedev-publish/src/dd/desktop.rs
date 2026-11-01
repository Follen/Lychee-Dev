//! Authorize the current DD/WebView2 role before obtaining a private cookie.
use super::{
    Installation,
    desktop_cookie::{self, Cookie},
};
use crate::desktop_runtime::{self, VerifiedImage};
use lycheedev_core::{Result, ToolkitError, control::OperationControl, storage};
use lycheedev_windows::{
    ProcessIdentity,
    credentials::{self, AppData},
    process::{
        ProcessLifetime,
        metadata::{self, Metadata, Row},
        observe_lifetime,
    },
    process_file_snapshot,
};
use std::{
    collections::BTreeSet,
    ffi::{OsStr, OsString},
    fs::File,
    io::Read,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

const GUI: &str = "netease_dd.exe";
const WEBVIEW: &str = "msedgewebview2.exe";
const NETWORK: &str = "--utility-sub-type=network.mojom.NetworkService";
const NETEASE: &[&str] = &["NetEase (Hangzhou) Network Co., Ltd"];
const MICROSOFT: &[&str] = &["Microsoft Corporation"];
pub(super) struct Desktop {
    pub gui: ProcessIdentity,
    pub network: ProcessIdentity,
    network_image: PathBuf,
    profile: PathBuf,
    pub installation: Installation,
}
/// Private helper metadata. It contains no cookie, token or managed address.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ChannelAnchor {
    gui: ProcessIdentity,
    network: ProcessIdentity,
    network_image: PathBuf,
    profile: PathBuf,
    program_dir: PathBuf,
    executable: PathBuf,
}
impl ChannelAnchor {
    pub(super) fn verify(&self, control: &OperationControl) -> Result<()> {
        let current = select(control)?.ok_or_else(|| fault("the original DD GUI session ended"))?;
        self.matches(&current.channel_anchor())
    }
    fn matches(&self, actual: &Self) -> Result<()> {
        if self != actual {
            return Err(fault(
                "the original DD GUI, browser profile or installation changed",
            ));
        }
        Ok(())
    }
    pub(super) fn gui(&self) -> &ProcessIdentity {
        &self.gui
    }
    pub(super) fn executable(&self) -> &Path {
        &self.executable
    }
    pub(super) fn program_dir(&self) -> &Path {
        &self.program_dir
    }
    pub(super) fn ccid(&self) -> Result<&str> {
        self.profile
            .parent()
            .and_then(Path::file_name)
            .and_then(OsStr::to_str)
            .filter(|v| !v.is_empty() && v.len() <= 30 && v.bytes().all(|b| b.is_ascii_digit()))
            .ok_or_else(|| {
                fault("DD channel reading requires the original numeric account profile")
            })
    }
}
fn fault(message: &str) -> ToolkitError {
    ToolkitError::new("publish.dd_desktop", message, 3)
}
fn native(error: String, control: &OperationControl) -> ToolkitError {
    control.check().err().unwrap_or_else(|| {let _=error; fault("DD process metadata or file snapshot is unavailable; no additional login was attempted")})
}
fn check(control: &OperationControl) -> impl FnMut() -> std::result::Result<(), String> + '_ {
    || {
        control
            .check()
            .map_err(|_| "DD desktop operation stopped".into())
    }
}
fn named(path: &Path, name: &str) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|value| value.eq_ignore_ascii_case(name))
}
pub(super) fn select(control: &OperationControl) -> Result<Option<Desktop>> {
    let rows = metadata::rows(check(control)).map_err(|e| native(e, control))?;
    if !rows.iter().any(|row| row.name.eq_ignore_ascii_case(GUI)) {
        return Ok(None);
    }
    let roaming = credentials::app_data(AppData::Roaming).map_err(|e| native(e, control))?;
    let root = roaming.join("CCVoiceHub/web_cache");
    storage::guard_path_with_check(&roaming, &root, || control.check())?;
    select_with(
        &rows,
        &root,
        control,
        |pid, args| metadata::inspect(pid, args, check(control)).map_err(|e| native(e, control)),
        |path, publishers| {
            desktop_runtime::verify_image(path, publishers, control).map_err(|error| {
                control.check().err().unwrap_or_else(|| {
                    let _ = error;
                    fault("DD or WebView2 does not have the required official publisher signature")
                })
            })
        },
    )
}
pub(super) fn require_absent(control: &OperationControl) -> Result<()> {
    let rows = metadata::rows(check(control)).map_err(|e| native(e, control))?;
    if rows.iter().any(|row| row.name.eq_ignore_ascii_case(GUI)) {
        return Err(fault(
            "an existing DD GUI requires its borrowed session; native login was not attempted",
        ));
    }
    Ok(())
}
fn profile_argument(arguments: &[OsString]) -> Result<PathBuf> {
    let prefix = "--user-data-dir=".encode_utf16().collect::<Vec<_>>();
    let values = arguments
        .iter()
        .filter_map(|arg| {
            let wide = arg.encode_wide().collect::<Vec<_>>();
            wide.strip_prefix(&prefix[..])
                .map(|value| PathBuf::from(OsString::from_wide(value)))
        })
        .collect::<Vec<_>>();
    if values.len() != 1 || !values[0].is_absolute() {
        return Err(fault(
            "DD network process has no unique absolute browser profile",
        ));
    }
    Ok(values.into_iter().next().unwrap())
}
fn select_with(
    rows: &[Row],
    root: &Path,
    control: &OperationControl,
    mut inspect: impl FnMut(u32, bool) -> Result<Metadata>,
    mut verify: impl FnMut(&Path, &[&str]) -> Result<VerifiedImage>,
) -> Result<Option<Desktop>> {
    control.check()?;
    let guis = rows
        .iter()
        .filter(|row| row.name.eq_ignore_ascii_case(GUI))
        .collect::<Vec<_>>();
    if guis.is_empty() {
        return Ok(None);
    }
    if guis.len() != 1 {
        return Err(fault(
            "DD requires one unique desktop process; select one desktop instance first",
        ));
    }
    let gui = inspect(guis[0].pid, false)?;
    if gui.identity.pid != guis[0].pid || !named(&gui.image, GUI) {
        return Err(fault("DD desktop process identity changed"));
    }
    let gui_image = verify(&gui.image, NETEASE)?;
    let root = root
        .canonicalize()
        .map_err(|_| fault("DD browser state directory is unavailable"))?;
    let mut descendants = BTreeSet::from([gui.identity.pid]);
    for _ in 0..8 {
        let next = descendants
            .iter()
            .copied()
            .chain(
                rows.iter()
                    .filter(|row| descendants.contains(&row.parent))
                    .map(|row| row.pid),
            )
            .collect::<BTreeSet<_>>();
        if next == descendants {
            break;
        }
        descendants = next;
    }
    let mut candidates = Vec::new();
    for row in rows
        .iter()
        .filter(|row| descendants.contains(&row.pid) && row.name.eq_ignore_ascii_case(WEBVIEW))
    {
        control.check()?;
        // Short-lived renderer processes may end during the OS snapshot. Only
        // a readable, exact network-role command line can select a profile.
        let metadata = match inspect(row.pid, true) {
            Ok(value) => value,
            Err(error) => {
                control.check()?;
                let _ = error;
                continue;
            }
        };
        if metadata.identity.pid != row.pid || !named(&metadata.image, WEBVIEW) {
            return Err(fault("DD browser process identity changed"));
        }
        let arguments = metadata
            .arguments
            .as_ref()
            .ok_or_else(|| fault("DD browser process arguments are unavailable"))?;
        if !arguments.iter().any(|arg| arg == OsStr::new(NETWORK)) {
            continue;
        }
        let raw = profile_argument(arguments)?;
        let profile = raw
            .canonicalize()
            .map_err(|_| fault("DD browser profile is unavailable"))?;
        if !named(&profile, "EBWebView") || !profile.starts_with(&root) || profile == root {
            return Err(fault(
                "DD browser profile is outside its product state directory",
            ));
        }
        storage::guard_path_with_check(&root, &profile, || control.check())?;
        verify(&metadata.image, MICROSOFT)?;
        if metadata.identity.created < gui.identity.created {
            return Err(fault(
                "DD browser lifetime does not belong to this desktop instance",
            ));
        }
        candidates.push((metadata, profile));
    }
    if candidates.len() != 1 {
        return Err(fault(
            "DD does not expose one unique active browser session; open DD and sign in first",
        ));
    }
    let current = inspect(gui.identity.pid, false)?;
    if current.identity != gui.identity || current.image != gui.image {
        return Err(fault(
            "DD desktop lifetime changed while selecting its browser session",
        ));
    }
    let (network, profile) = candidates.pop().unwrap();
    let program_dir = gui_image
        .path
        .parent()
        .ok_or_else(|| fault("DD desktop installation path is invalid"))?
        .to_owned();
    Ok(Some(Desktop {
        gui: gui.identity,
        network: network.identity,
        network_image: network.image,
        profile,
        installation: Installation {
            program_dir,
            executable: gui_image.path,
            version: gui_image.version,
        },
    }))
}
impl Desktop {
    pub(super) fn channel_anchor(&self) -> ChannelAnchor {
        ChannelAnchor {
            gui: self.gui.clone(),
            network: self.network.clone(),
            network_image: self.network_image.clone(),
            profile: self.profile.clone(),
            program_dir: self.installation.program_dir.clone(),
            executable: self.installation.executable.clone(),
        }
    }
    #[cfg(test)]
    pub(super) fn synthetic(gui: ProcessIdentity, network: ProcessIdentity) -> Self {
        Self {
            gui,
            network,
            network_image: PathBuf::from("synthetic-webview.exe"),
            profile: PathBuf::from("1234/EBWebView"),
            installation: Installation {
                program_dir: PathBuf::from("synthetic-program"),
                executable: PathBuf::from("synthetic-program/netease_dd.exe"),
                version: "synthetic".into(),
            },
        }
    }
    pub(super) fn cookie(&self, control: &OperationControl) -> Result<Cookie> {
        control.check()?;
        let database = self.profile.join("Default/Network/Cookies");
        let wal = self.profile.join("Default/Network/Cookies-wal");
        storage::guard_path_with_check(&self.profile, &database, || control.check())?;
        storage::guard_path_with_check(&self.profile, &wal, || control.check())?;
        let wal_exists = wal
            .try_exists()
            .map_err(|_| fault("DD browser WAL presence could not be checked"))?;
        let snapshot = process_file_snapshot::snapshot_process_files(
            &self.network,
            &self.network_image,
            &[&database, &wal],
            check(control),
        )
        .map_err(|e| native(e, control))?;
        if wal_exists && snapshot.get(1).is_none() {
            return Err(fault(
                "DD browser WAL is not available from the original network process",
            ));
        }
        let state = self.profile.join("Local State");
        storage::guard_path_with_check(&self.profile, &state, || control.check())?;
        let mut file =
            File::open(&state).map_err(|_| fault("DD browser Local State is unavailable"))?;
        let read = (|| {
            let mut bytes = Vec::new();
            (&mut file)
                .take((1 << 20) + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| fault("DD browser Local State could not be read"))?;
            control.check()?;
            if bytes.len() > 1 << 20 {
                return Err(fault("DD browser Local State exceeds its budget"));
            }
            Ok(bytes)
        })();
        let state = storage::finish_file(file, read, control)?;
        if observe_lifetime(&self.gui, check(control)).map_err(|e| native(e, control))?
            != ProcessLifetime::Running
        {
            return Err(fault(
                "the original DD desktop session ended while borrowing its cookie",
            ));
        }
        let now = chrono::Utc::now()
            .timestamp_micros()
            .checked_add(11_644_473_600_000_000)
            .ok_or_else(|| fault("current browser expiry time is invalid"))?;
        desktop_cookie::decode(
            snapshot
                .get(0)
                .ok_or_else(|| fault("DD browser has no readable session database"))?,
            snapshot.get(1).unwrap_or(&[]),
            &state,
            now,
            control,
        )
    }
}

#[cfg(test)]
mod tests;
