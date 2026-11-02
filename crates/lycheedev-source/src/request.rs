//! Typed inputs shared by Source operations and their execution owners.
use lycheedev_core::control::OperationControl;
use lycheedev_core::selection::{PinnedSet, SourcePin};
use lycheedev_core::{Result, ToolkitError};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct SourceContext {
    pub home: PathBuf,
    pub cwd: PathBuf,
    pub control: OperationControl,
}

/// An admitted immutable selection that contains a Source identity.
/// The CLI resolves projects and persisted pins before constructing this value.
#[derive(Clone, Debug)]
pub struct SelectedSource {
    pin: PinnedSet,
}

impl SelectedSource {
    pub fn new(pin: PinnedSet) -> Result<Self> {
        if pin.source.is_none() {
            return Err(ToolkitError::new(
                "selection.source_missing",
                "Pinned selection has no source",
                3,
            ));
        }
        Ok(Self { pin })
    }

    pub fn pin(&self) -> &PinnedSet {
        &self.pin
    }

    pub fn snapshot(&self) -> &str {
        &self.pin.id
    }

    pub fn source(&self) -> &SourcePin {
        self.pin.source.as_ref().expect("admitted Source selection")
    }
}

#[derive(Clone, Debug)]
pub struct SyncRequest {
    pub repository: String,
    pub product: String,
    pub reference: String,
}

#[derive(Clone, Debug)]
pub enum InspectTarget {
    Excerpt {
        path: String,
        /// Positive one-based line number admitted by the caller.
        first_line: usize,
        /// Positive excerpt length, bounded to 2,000 lines by CLI admission.
        line_count: usize,
    },
    Symbol(String),
    TargetPath(String),
}

impl InspectTarget {
    pub fn locator(&self) -> &str {
        match self {
            Self::Excerpt { path, .. } | Self::Symbol(path) | Self::TargetPath(path) => path,
        }
    }
}

#[derive(Clone, Debug)]
pub struct QueryRequest {
    pub query: String,
    pub mode: String,
    pub topic: String,
    pub limit: usize,
    pub cursor: String,
}

#[derive(Clone, Debug)]
pub struct AddonValidationRequest {
    pub selection: SelectedSource,
    pub root: PathBuf,
    pub toc: String,
    pub semantic: bool,
    pub environment: Option<SourcePin>,
    /// Original addon path and TOC, retained as the capture's display locator.
    pub locator: String,
}

#[derive(Clone, Debug)]
pub struct MatrixValidationRequest {
    pub configuration: PathBuf,
    pub root: PathBuf,
    pub targets: Vec<crate::matrix::Target>,
    pub semantic: bool,
}

#[derive(Clone, Debug)]
pub struct DiffRequest {
    pub from: SelectedSource,
    pub to: SelectedSource,
    pub limit: usize,
}

/// Ordinary Source operations after command-line admission. Research batches
/// and native sessions have their own request and owner types below.
#[derive(Clone, Debug)]
pub enum SourceRequest {
    Catalog,
    Status(SelectedSource),
    Sync(SyncRequest),
    Inspect {
        selection: SelectedSource,
        target: InspectTarget,
    },
    Index(SelectedSource),
    Query {
        selection: SelectedSource,
        request: QueryRequest,
    },
    Validate(AddonValidationRequest),
    Matrix(MatrixValidationRequest),
    Diff(DiffRequest),
    Prune {
        target_bytes: u64,
    },
}

#[derive(Clone, Debug)]
pub struct ResearchSelection {
    pub pin: PinnedSet,
    pub environment: Option<PinnedSet>,
}

impl ResearchSelection {
    pub fn source(&self) -> Result<&SourcePin> {
        self.pin.source.as_ref().ok_or_else(|| {
            ToolkitError::new("selection.source_missing", "selected pin has no source", 3)
        })
    }

    pub(crate) fn environment_source(&self) -> Result<Option<&SourcePin>> {
        match &self.environment {
            Some(pin) => pin.source.as_ref().map(Some).ok_or_else(|| {
                ToolkitError::new(
                    "selection.source_missing",
                    "selected environment pin has no source",
                    3,
                )
            }),
            None => {
                let source = self.source()?;
                Ok((source.repository == "wow-ui-source").then_some(source))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResearchMode {
    Static,
    #[default]
    OneShot,
    Session,
}

impl ResearchMode {
    pub fn uses_native(self) -> bool {
        self != Self::Static
    }

    pub fn reuses_session(self) -> bool {
        self == Self::Session
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SymbolSelector {
    Id(String),
    Name(String),
}

impl SymbolSelector {
    pub fn value(&self) -> &str {
        match self {
            Self::Id(value) | Self::Name(value) => value,
        }
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Id(value) => Some(value),
            Self::Name(_) => None,
        }
    }

    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Name(value) => Some(value),
            Self::Id(_) => None,
        }
    }

    pub(crate) fn matches(&self, symbol: &crate::index::Symbol) -> bool {
        match self {
            Self::Id(value) => *value == symbol.id,
            Self::Name(value) => *value == symbol.name,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RelationDirection {
    Incoming,
    Outgoing,
    #[default]
    Both,
}

impl RelationDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Incoming => "incoming",
            Self::Outgoing => "outgoing",
            Self::Both => "both",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RelationRequest {
    pub selector: SymbolSelector,
    pub direction: RelationDirection,
    pub limit: usize,
    pub cursor: String,
}

#[derive(Clone, Debug)]
pub struct ContextRequest {
    pub selector: SymbolSelector,
    pub limit: usize,
    pub max_bytes: usize,
    pub max_lines: usize,
    pub depth: usize,
    pub cursor: String,
    pub flow: bool,
}

impl ContextRequest {
    pub(crate) fn relations(&self, cursor: String) -> RelationRequest {
        RelationRequest {
            selector: self.selector.clone(),
            direction: RelationDirection::Both,
            limit: self.limit,
            cursor,
        }
    }
}
