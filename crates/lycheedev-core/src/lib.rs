#![forbid(unsafe_code)]
//! Shared contracts and durable mechanisms, without dependencies on business crates.
//! Business dependencies remain directed and acyclic: CLI composes the domains,
//! while Live uses Delivery for controlled installation of managed mailbox slots.
//! Source and Data do not depend on each other or on Live/Delivery.
pub mod archive;
pub mod availability;
pub mod cache;
pub mod client;
pub mod control;
pub mod coordination;
pub mod doctor;
pub mod evidence;
pub mod installation_selection;
pub mod mailbox;
pub mod metadata;
pub mod process;
pub mod public_json;
pub mod raw_json;
pub mod request;
pub mod selection;
pub mod storage;
pub mod workspace;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub type Result<T> = std::result::Result<T, ToolkitError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolkitError {
    pub code: String,
    pub message: String,
    pub exit_code: i32,
    pub details: Value,
}
impl ToolkitError {
    pub fn new(code: impl Into<String>, message: impl Into<String>, exit_code: i32) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            exit_code,
            details: Value::Null,
        }
    }
    pub fn arguments(message: impl Into<String>) -> Self {
        Self::new("command.invalid_arguments", message, 2)
    }
    pub fn io(error: std::io::Error) -> Self {
        Self::new("external.io", error.to_string(), 5)
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("input.invalid", message, 4)
    }
    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }
}
impl std::fmt::Display for ToolkitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for ToolkitError {}
impl From<std::io::Error> for ToolkitError {
    fn from(value: std::io::Error) -> Self {
        Self::io(value)
    }
}
impl From<serde_json::Error> for ToolkitError {
    fn from(value: serde_json::Error) -> Self {
        Self::invalid(value.to_string())
    }
}
impl From<rusqlite::Error> for ToolkitError {
    fn from(value: rusqlite::Error) -> Self {
        Self::new("vault.metadata", value.to_string(), 5)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Outcome {
    pub data: Value,
    /// Encoding-only typed provenance; never persisted in result artifacts.
    #[serde(skip)]
    pub public_numbers: public_json::NumberLayout,
    pub complete: bool,
    pub warnings: Vec<Value>,
    pub selection: Value,
    pub coverage: Value,
    pub provenance: Vec<Value>,
    pub operation: Value,
}
impl Outcome {
    pub fn new(data: Value) -> Self {
        Self {
            data,
            public_numbers: public_json::NumberLayout::Verbatim,
            complete: true,
            warnings: vec![],
            selection: json!({}),
            coverage: json!({}),
            provenance: vec![],
            operation: Value::Null,
        }
    }
}
