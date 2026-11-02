//! Parsed command input owned exclusively by the CLI.
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone)]
pub struct Invocation {
    pub command: String,
    pub positional: Option<String>,
    pub flags: BTreeMap<String, String>,
    pub home: PathBuf,
    pub cwd: PathBuf,
    pub control: OperationControl,
}
impl Invocation {
    pub fn flag_values(&self, name: &str) -> Vec<String> {
        self.flags
            .get(&format!("__repeat:{name}"))
            .and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or_else(|| {
                self.flag(name)
                    .map(|value| vec![value.to_owned()])
                    .unwrap_or_default()
            })
    }
    pub fn flag(&self, name: &str) -> Option<&str> {
        self.flags.get(name).map(String::as_str)
    }
    pub fn flag_bool(&self, name: &str) -> bool {
        self.flag(name).is_some_and(|value| value == "true")
    }
    pub fn required(&self, name: &str) -> Result<&str> {
        self.flag(name)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ToolkitError::arguments(format!("{} requires {}", self.command, name)))
    }
    pub fn argument(&self) -> Result<&str> {
        self.positional.as_deref().ok_or_else(|| {
            ToolkitError::arguments(format!("{} requires an argument", self.command))
        })
    }
    pub fn usize_flag(&self, name: &str, default: usize, min: usize, max: usize) -> Result<usize> {
        let value = self
            .flag(name)
            .map(|s| s.parse::<usize>())
            .transpose()
            .map_err(|_| ToolkitError::arguments(format!("{name} requires an integer")))?
            .unwrap_or(default);
        if !(min..=max).contains(&value) {
            return Err(ToolkitError::arguments(format!(
                "{name} must be in {min}..={max}"
            )));
        }
        Ok(value)
    }
}
