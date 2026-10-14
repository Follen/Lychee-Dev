//! Read-only discovery. Its source exposes window metadata, never a native target.
use lycheedev_core::{
    Invocation, Result, ToolkitError, client,
    control::OperationControl,
    coordination::ClaimRegistry,
    installation_selection::{self, InvocationMetadata},
    storage,
};
use lycheedev_windows::inventory::Window;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

/// Both native and fixture sources enter exactly the same discovery pipeline.
pub trait Source {
    fn windows(&self, control: &OperationControl) -> Result<Vec<Window>>;
}
pub struct WindowsSource;
impl Source for WindowsSource {
    fn windows(&self, control: &OperationControl) -> Result<Vec<Window>> {
        let result = lycheedev_windows::inventory::passive_windows(|| {
            control.check().map_err(|error| error.to_string())
        });
        control.check()?;
        result.map_err(|error| ToolkitError::new("command.failed", error, 5))
    }
}

pub use lycheedev_core::installation_selection::Client;
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowIdentity {
    pub handle: String,
    pub process_id: u32,
    pub process_started_at: String,
    pub executable: String,
    pub class: String,
    pub title: String,
}
impl From<&Window> for WindowIdentity {
    fn from(window: &Window) -> Self {
        Self {
            handle: window.identity.hwnd.to_string(),
            process_id: window.identity.process.pid,
            process_started_at: window.identity.process.created.to_string(),
            executable: window.executable.clone(),
            class: window.class.clone(),
            title: window.title.clone(),
        }
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub window: WindowIdentity,
    pub client: Client,
    pub state: String,
    pub input_ready: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub not_ready_reason: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub busy_operation_id: String,
    #[serde(skip_serializing_if = "is_false")]
    pub foreign_owner: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Installation {
    pub client: Client,
    pub state: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub installations: Vec<Installation>,
    // The frozen public Go report emits null when no candidate was appended.
    pub candidates: Option<Vec<Candidate>>,
}
fn is_false(value: &bool) -> bool {
    !*value
}
fn reason(error: &ToolkitError) -> String {
    if error.code == error.message
        || error.code == "external.io"
        || error.message.starts_with(&format!("{}:", error.code))
    {
        error.message.clone()
    } else {
        error.to_string()
    }
}

fn inspect(directory: &Path, control: &OperationControl) -> Result<Client> {
    installation_selection::select_with(directory, "", &mut InvocationMetadata(control))
        .map(Into::into)
}
fn path_key(path: &Path) -> String {
    client::display_path(&fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
        .to_lowercase()
}
fn failure(code: &str) -> ToolkitError {
    ToolkitError::new(code, code, 4)
}
fn window_client(window: &Window, control: &OperationControl) -> Result<Client> {
    if window.identity.hwnd == 0
        || window.identity.process.pid == 0
        || window.identity.process.created == 0
        || window.class.is_empty()
        || !Path::new(&window.executable).is_absolute()
    {
        return Err(failure("live.invalid_window_identity"));
    }
    control.check()?;
    // Resolve file identity without opening or reading the executable.
    let executable = fs::canonicalize(&window.executable)?;
    inspect(
        executable
            .parent()
            .ok_or_else(|| failure("live.invalid_window_identity"))?,
        control,
    )
}

/// Default and explicit --passive are deliberately the same operation.
pub fn discover(inv: &Invocation, source: &impl Source) -> Result<Report> {
    discover_inner(inv, source).map_err(|mut error| {
        if error.code == "command.deadline" {
            error.code = "live.deadline_pending".into();
            error.exit_code = 6;
        }
        error
    })
}
fn discover_inner(inv: &Invocation, source: &impl Source) -> Result<Report> {
    let control = &inv.control;
    control.check()?;
    let windows = source.windows(control)?;
    control.check()?;
    if windows.len() > 4096 {
        return Err(ToolkitError::new(
            "command.failed",
            "window.inventory_truncated",
            5,
        ));
    }
    let workspace_id = workspace_identity(&inv.home, control);
    control.check()?;
    let mut report = Report {
        installations: Vec::new(),
        candidates: None,
    };
    let mut running = BTreeSet::new();
    let mut running_dirs = Vec::new();
    for window in windows {
        control.check()?;
        let name = Path::new(&window.executable)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        if !window.visible || !["wow.exe", "wowclassic.exe", "wowb.exe"].contains(&name.as_str()) {
            continue;
        }
        let mut candidate = Candidate {
            window: (&window).into(),
            client: Client::default(),
            state: String::new(),
            input_ready: false,
            not_ready_reason: String::new(),
            busy_operation_id: String::new(),
            foreign_owner: false,
            reason: String::new(),
        };
        match window_client(&window, control) {
            Err(error) => {
                control.check()?;
                candidate.state = "identity_unreadable".into();
                candidate.reason = reason(&error);
            }
            Ok(client) => {
                if running.insert(path_key(Path::new(&client.directory))) {
                    running_dirs.push(PathBuf::from(&client.directory));
                }
                match ownership(&inv.home, &workspace_id, &client, &window, control) {
                    Ok(Some(owner)) => {
                        candidate.state = "occupied".into();
                        candidate.busy_operation_id = owner.operation;
                        candidate.foreign_owner = owner.foreign;
                    }
                    Ok(None) => {
                        candidate.state = "unobserved".into();
                        candidate.not_ready_reason = "passive_discovery".into();
                    }
                    Err(error) => {
                        control.check()?;
                        candidate.state = "occupied".into();
                        candidate.foreign_owner = true;
                        candidate.reason = reason(&error);
                    }
                }
                candidate.client = client;
            }
        }
        report
            .candidates
            .get_or_insert_with(Vec::new)
            .push(candidate);
    }
    let mut directories = Vec::new();
    if let Some(root) = inv.flag("--installation").filter(|root| !root.is_empty()) {
        directories.push(PathBuf::from(root));
        directories.extend(
            client::client_folders()
                .iter()
                .map(|folder| Path::new(root).join(folder)),
        );
    }
    directories.extend(running_dirs);
    let mut seen = BTreeSet::new();
    for directory in directories {
        control.check()?;
        let input_key = path_key(&directory);
        if seen.contains(&input_key) {
            continue;
        }
        let inspected = inspect(&directory, control);
        control.check()?;
        if let Ok(client) = inspected {
            // Fixed Go deduplicates the input scope, not the selected client.
            // A sole client selected from its root and again from its known
            // folder therefore remains two installation observations.
            seen.insert(input_key);
            let key = path_key(Path::new(&client.directory));
            let state = if running.contains(&key) {
                "running"
            } else {
                "installed"
            };
            report.installations.push(Installation {
                client,
                state: state.into(),
            });
        }
    }
    report
        .installations
        .sort_by_key(|item| item.client.directory.to_lowercase());
    control.check()?;
    Ok(report)
}

fn read_optional(path: &Path, limit: u64, control: &OperationControl) -> Result<Option<Vec<u8>>> {
    control.check()?;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    control.check()?;
    if bytes.len() as u64 > limit {
        return Err(failure("journal.invalid_window_owner"));
    }
    Ok(Some(bytes))
}
fn workspace_identity(home: &Path, control: &OperationControl) -> String {
    let read = || -> Result<String> {
        let Some(bytes) = read_optional(&home.join("workspace.json"), 64 << 10, control)? else {
            return Ok(String::new());
        };
        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
        let id = value["workspaceId"].as_str().unwrap_or("");
        if value["schema"] == "lycheedev.workspace.v1"
            && hex_string(id, 32)
            && value["createdAt"]
                .as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .is_some()
        {
            Ok(id.into())
        } else {
            Ok(String::new())
        }
    };
    read().unwrap_or_default()
}
fn hex_string(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowOwner {
    schema: String,
    workspace_id: String,
    resource: String,
    operation_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    intent_sha256: String,
}
struct Owner {
    operation: String,
    foreign: bool,
}
fn ownership(
    home: &Path,
    workspace_id: &str,
    client: &Client,
    window: &Window,
    control: &OperationControl,
) -> Result<Option<Owner>> {
    let parent = Path::new(&client.directory).join("Interface/AddOns");
    let parent = fs::canonicalize(&parent).unwrap_or(parent);
    let resource = format!(
        "window/{}/{}/{}",
        window.identity.process.pid, window.identity.process.created, window.identity.hwnd
    );
    let marker = parent
        .join(".lycheedev-window-owners")
        .join(format!("{}.json", storage::sha256(resource.as_bytes())));
    control.check()?;
    match fs::symlink_metadata(&marker) {
        Ok(metadata) => {
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || path_key(&marker) != client::display_path(&marker).to_lowercase()
            {
                return Err(failure("journal.redirected_window_owner"));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    if let Some(bytes) = read_optional(&marker, 4096, control)? {
        let owner: WindowOwner = serde_json::from_slice(&bytes)?;
        let id = ["OP-", "BTP-", "CON-"]
            .iter()
            .find_map(|prefix| owner.operation_id.strip_prefix(prefix));
        if storage::identity_json(&owner)? != bytes
            || owner.schema != "lycheedev.window-owner.v1"
            || owner.resource != resource
            || !hex_string(&owner.workspace_id, 32)
            || id.is_none_or(|id| !hex_string(id, 32))
            || (!owner.intent_sha256.is_empty() && !hex_string(&owner.intent_sha256, 64))
        {
            return Err(failure("journal.invalid_window_owner"));
        }
        return Ok(Some(Owner {
            operation: owner.operation_id,
            foreign: owner.workspace_id != workspace_id,
        }));
    }
    // Current Rust native sessions use installation claims. Inspect their metadata
    // directly: acquiring InstallationGuard would create locks and mutate state.
    if let Some(bytes) = read_optional(&parent.join(".lycheedev-claims.json"), 4 << 20, control)? {
        let registry: ClaimRegistry = serde_json::from_slice(&bytes)?;
        if registry.schema != "lycheedev.claims.v1" || registry.claims.len() > 1024 {
            return Err(failure("coordination.claim_format"));
        }
        let resource = format!(
            "process:{}:{}",
            window.identity.process.pid, window.identity.process.created
        );
        let mut matches = registry
            .claims
            .iter()
            .filter(|claim| claim.pending && claim.resource == resource);
        if let Some(claim) = matches.next() {
            if matches.next().is_some()
                || claim.kind != "native-live"
                || !hex_string(&claim.owner, 32)
                || claim.process_id != window.identity.process.pid
                || claim.created_at != window.identity.process.created.to_string()
            {
                return Err(failure("coordination.claim_format"));
            }
            let expected = home
                .join("live/v2")
                .join(format!("CON-{}.json", claim.owner));
            return Ok(Some(Owner {
                operation: format!("CON-{}", claim.owner),
                foreign: workspace_id.is_empty()
                    || path_key(Path::new(&claim.lifecycle_ref)) != path_key(&expected),
            }));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
