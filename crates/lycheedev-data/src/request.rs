//! Typed request context shared by the Data engine and authenticated readers.
use lycheedev_core::{Result, control::OperationControl, selection::{self, PinnedSet}};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};

#[derive(Clone, Debug)]
pub struct CachePrepareRequest {
    pub table: String,
    pub timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct DataContext {
    pub home: PathBuf,
    pub cwd: PathBuf,
    pub control: OperationControl,
    pub source: SourceOptions,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceOptions {
    pub snapshot: Option<String>,
    pub project: Option<String>,
    pub cdn: bool,
    pub installation: Option<PathBuf>,
    pub offline: bool,
    pub key_file: Option<String>,
    pub content_variant: ContentVariant,
    pub max_file_bytes: usize,
}
impl Default for SourceOptions {
    fn default() -> Self {
        Self {
            snapshot: None,
            project: None,
            cdn: false,
            installation: None,
            offline: false,
            key_file: None,
            content_variant: ContentVariant::Unspecified,
            max_file_bytes: 128 << 20,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentVariant {
    #[serde(rename = "")]
    #[default]
    Unspecified,
    #[serde(rename = "standard")]
    Standard,
    #[serde(rename = "low-violence")]
    LowViolence,
}
impl ContentVariant {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unspecified => "",
            Self::Standard => "standard",
            Self::LowViolence => "low-violence",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionConfig {
    pub max_handles: usize,
    pub retained_bytes: usize,
    pub result_ttl_seconds: u64,
}
impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_handles: 32,
            retained_bytes: 512 << 20,
            result_ttl_seconds: 300,
        }
    }
}

impl DataContext {
    pub fn resolve_pinned_with_check(
        &self,
        supplemental: impl FnMut() -> Result<()>,
    ) -> Result<PinnedSet> {
        selection::resolve_pin_request_controlled_with_check(
            selection::PinSelectionRequest {
                home: &self.home,
                cwd: &self.cwd,
                snapshot: self.source.snapshot.as_deref(),
                project: self.source.project.as_deref(),
            },
            &self.control,
            supplemental,
        )
    }

    pub fn resolve_pinned(&self) -> Result<PinnedSet> {
        self.resolve_pinned_with_check(|| Ok(()))
    }
}
