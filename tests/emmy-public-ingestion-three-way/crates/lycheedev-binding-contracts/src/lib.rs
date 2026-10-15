#![forbid(unsafe_code)]
//! Engine-independent contracts for frozen Lua/XML binding evidence.
//!
//! This crate owns evidence values, not engine syntax, type inference, graph
//! traversal, admission, or execution. Emmy re-exports these exact types for
//! compatibility; adapters can depend directly on them without an engine cycle.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum BindingDecision<T> {
    NotManaged,
    Bound(T),
    KnownAbsent { proof: String },
    Unavailable { proof: String, reason: String },
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ObjectViewKey {
    pub context: String,
    pub observation: usize,
    pub object_id: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldPresence {
    Present,
    Absent,
    Unavailable,
}
#[derive(Clone, Debug)]
pub enum ShapeEnumeration {
    Complete,
    Unavailable(String),
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSlot {
    pub index: usize,
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default)]
    pub inner_type: String,
    #[serde(default)]
    pub nilable: bool,
    #[serde(default)]
    pub line: usize,
    #[serde(default)]
    pub raw: BTreeMap<String, serde_json::Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnvironmentOwner {
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub source: serde_json::Value,
}
/// The actual ApiRecord schema: parameters/returns/payload/fields/values carry
/// Slot records; raw metadata is retained and is never interpreted as Lua code.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentRecord {
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub owner: Option<EnvironmentOwner>,
    #[serde(default)]
    pub declaration_role: String,
    #[serde(default)]
    pub display_group: String,
    #[serde(default)]
    pub namespace: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub line: usize,
    #[serde(default)]
    pub end_line: usize,
    #[serde(default)]
    pub parameters: Vec<EnvironmentSlot>,
    #[serde(default)]
    pub returns: Vec<EnvironmentSlot>,
    #[serde(default)]
    pub payload: Vec<EnvironmentSlot>,
    #[serde(default)]
    pub fields: Vec<EnvironmentSlot>,
    #[serde(default)]
    pub values: Vec<EnvironmentSlot>,
    #[serde(default)]
    pub raw: BTreeMap<String, serde_json::Value>,
}
#[derive(Clone, Debug, Default)]
pub struct EnvironmentTypeCatalog {
    pub identity: String,
    pub aliases: BTreeMap<String, String>,
    pub nominal_objects: BTreeMap<String, serde_json::Value>,
    pub declarations: BTreeMap<String, Vec<EnvironmentRecord>>,
}
#[derive(Clone, Debug)]
pub struct EnvironmentOrigin {
    pub material: String,
    pub material_sha256: String,
    pub pointer: String,
    pub identity: String,
}
#[derive(Clone, Debug)]
pub struct EnvironmentCallable {
    pub origin: EnvironmentOrigin,
    pub declaration: EnvironmentRecord,
    pub types: Arc<EnvironmentTypeCatalog>,
}
impl EnvironmentCallable {
    pub fn nominal_return_name(&self, slot: usize) -> Option<&str> {
        let value = self.declaration.returns.get(slot.checked_sub(1)?)?;
        if value.index != slot || value.nilable {
            return None;
        }
        self.types
            .nominal_objects
            .contains_key(&value.type_name)
            .then_some(value.type_name.as_str())
    }
    pub fn receiver_type_name(&self) -> Option<&str> {
        let owner = self
            .declaration
            .owner
            .as_ref()
            .filter(|o| o.kind == "script-object")?;
        let mut names = self.types.nominal_objects.iter().filter(|(_, value)| {
            value.get("owner").and_then(serde_json::Value::as_str) == Some(owner.name.as_str())
        });
        let (name, _) = names.next()?;
        if names.next().is_some() {
            return None;
        }
        Some(name.as_str())
    }
}
#[derive(Clone, Debug)]
pub struct EnvironmentReturn {
    pub callable: Arc<EnvironmentCallable>,
    pub slot: usize,
    pub invocation_identity: String,
}
