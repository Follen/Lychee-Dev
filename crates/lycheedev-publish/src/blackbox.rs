//! Heybox Workshop's explicit HTTP author session and native publishing API.
//! SMS/WeChat selection is independent of the desktop client's logged-in user.
use lycheedev_core::{Result, ToolkitError, control::OperationControl, storage};
use serde_json::{Value, json};
use std::path::Path;
mod auth;
mod backup;
mod cos;
mod forms;
mod plugin;
mod protocol;
mod read;
mod write;
pub use auth::{AuthAction, AuthWrite};
pub use backup::{BackupCreate, BackupGenerate, BackupUpload, BackupWrite, scan_backup};
pub use read::Query;
pub use write::Write;

fn protocol_error(message: &str) -> ToolkitError {
    ToolkitError::new("publish.blackbox_protocol", message, 4)
}
fn login_required() -> ToolkitError {
    ToolkitError::new(
        "publish.blackbox_login_required",
        "Heybox login is required; use publish blackbox session login-start",
        3,
    )
}
fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
fn random() -> Result<String> {
    lycheedev_windows::random::token().map_err(|_| {
        ToolkitError::new(
            "publish.blackbox_random",
            "cannot generate a Heybox request identity",
            3,
        )
    })
}
pub fn login_status(home: &Path, id: &str, control: &OperationControl) -> Result<Value> {
    auth::validate_login_id(id)?;
    auth::Auth::open(home, control)?.finish(|auth| auth.status(id))
}
pub fn doctor(home: &Path, control: &OperationControl) -> Result<Value> {
    // Renewal is an explicit effect of this provider's readiness check.
    auth::Auth::open(home, control)?.finish(|auth| auth.doctor())
}
pub fn read(home: &Path, query: Query, control: &OperationControl) -> Result<Value> {
    query.validate()?;
    auth::Auth::open(home, control)?.finish(|auth| {
        let mut session = auth.renew(false)?;
        let result = read::execute(&auth.api, &mut session, query);
        result.map(|value| crate::output::public(value, &["description", "Article", "UpdateLog"]))
    })
}

#[cfg(test)]
mod tests;
