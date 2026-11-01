//! Task-scoped publisher owner. Each command attaches by explicit session ID;
//! only an explicit start may create a new owner and native connection.
use super::worker::{
    self, Call, Worker, check, now_ms, operation, read_frame, send, wire_deadline,
};
use lycheedev_core::{
    Result, ToolkitError, control::OperationControl, metadata::owned_write::finish_owned, storage,
};
use lycheedev_windows::{
    Process, ProcessIdentity,
    broker::{BrokerListener, BrokerScope, BrokerStream},
    process::{ProcessLifetime, observe_lifetime},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

mod client;
mod server;
mod store;
pub use client::Client;
pub use server::entry;
use store::Store;
const WIRE: &str = "lycheedev.publish.dd-task.v1";
const SCHEMA: &str = "lycheedev.publish.dd-task-state.v1";
const START_ENV: &str = "LYCHEEDEV_DD_TASK_START";
const IDLE: Duration = Duration::from_secs(600);
const REAP: Duration = Duration::from_secs(5);
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Starting,
    Ready,
    ResourcesReleased,
    Failed,
    Interrupted,
    CleanupPending,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Last {
    id: String,
    input_sha256: String,
    deadline_ms: u64,
    mutation: bool,
    outcome: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    id: String,
    digest: String,
    executable: PathBuf,
    parent: ProcessIdentity,
    start_deadline_ms: u64,
    owner: Option<ProcessIdentity>,
    worker: Option<ProcessIdentity>,
    phase: Phase,
    identity: Value,
    last: Option<Last>,
    error: Option<ToolkitError>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Hello {
    wire: String,
    session: String,
    owner: ProcessIdentity,
    digest: String,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "task", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    Activate,
    Status,
    Stop,
    Execute { call: Call },
}
impl Action {
    fn mutation(&self) -> bool {
        match self {
            Self::Execute { call } => call.mutates(),
            _ => false,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    wire: String,
    session: String,
    id: String,
    deadline_ms: u64,
    action: Action,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    wire: String,
    session: String,
    id: String,
    result: Result<Value>,
}
fn fault(message: &str) -> ToolkitError {
    ToolkitError::new("publish.dd_task", message, 3)
}
fn invalid() -> ToolkitError {
    ToolkitError::new(
        "publish.dd_task_protocol",
        "invalid DD task identity or private request",
        4,
    )
}
fn platform(_: String) -> ToolkitError {
    fault("DD task process or transport operation failed")
}
fn token() -> Result<String> {
    lycheedev_windows::random::token().map_err(platform)
}
fn hex(value: &str, n: usize) -> bool {
    value.len() == n
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn session_id(value: &str) -> bool {
    value.strip_prefix("DDT-").is_some_and(|s| hex(s, 32))
}
/// Validate a public selector before opening home, processes or transports.
pub fn validate_session_id(value: &str) -> Result<()> {
    if session_id(value) {
        Ok(())
    } else {
        Err(ToolkitError::arguments(
            "DD session requires the DDT- identity returned by session start (32 lowercase hexadecimal characters after DDT-)",
        ))
    }
}
fn startup_pending(record: &Record) -> Result<bool> {
    Ok(record.phase == Phase::Starting
        && record.owner.is_none()
        && record.start_deadline_ms > now_ms()?)
}
fn pending(record: &Record) -> ToolkitError {
    ToolkitError::new(
        "publish.dd_task_pending",
        "the original DD task startup is still awaiting its owner",
        6,
    )
    .with_details(view(record, false))
}
fn current() -> Result<ProcessIdentity> {
    let process = Process::open(std::process::id()).map_err(platform)?;
    let id = process.identity().clone();
    process.close().map_err(platform)?;
    Ok(id)
}
fn ended(identity: &ProcessIdentity) -> Result<bool> {
    observe_lifetime(identity, || Ok(()))
        .map(|s| s != ProcessLifetime::Running)
        .map_err(platform)
}
fn pin(executable: &Path, control: &OperationControl) -> Result<(File, String)> {
    storage::locked_sha256(executable, 512 << 20, control)
}
fn view(record: &Record, running: bool) -> Value {
    json!({"session_id":record.id,"running":running,"phase":record.phase,"owner":record.owner,"worker":record.worker,"identity":record.identity,
        "last_operation":record.last,"error":record.error,"replay_allowed":false})
}

trait Backend {
    fn identity(&self) -> Value;
    fn execute(&mut self, call: Call, control: &OperationControl) -> Result<Value>;
    fn retired(&self) -> bool;
    fn close(self, control: &OperationControl) -> Result<()>;
}
impl Backend for Worker {
    fn identity(&self) -> Value {
        self.identity().clone()
    }
    fn execute(&mut self, call: Call, control: &OperationControl) -> Result<Value> {
        self.call(call, control)
    }
    fn retired(&self) -> bool {
        self.is_retired()
    }
    fn close(self, control: &OperationControl) -> Result<()> {
        self.close(control).map(|_| ())
    }
}

#[cfg(test)]
mod tests;
