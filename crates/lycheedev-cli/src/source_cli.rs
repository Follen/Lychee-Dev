//! Command-line admission for Source research. Domain operations receive only
//! typed queries, fixed selections and their execution context.
use crate::Invocation;
use lycheedev_core::{Outcome, Result, ToolkitError, selection};
use lycheedev_source::{
    SourceTools, batch,
    request::{
        AddonValidationRequest, ContextRequest, DiffRequest, InspectTarget,
        MatrixValidationRequest, QueryRequest, RelationDirection, RelationRequest, ResearchMode,
        ResearchSelection, SelectedSource, SourceContext, SourceRequest, SymbolSelector,
        SyncRequest,
    },
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub(crate) fn context(inv: &Invocation) -> SourceContext {
    SourceContext {
        home: inv.home.clone(),
        cwd: inv.cwd.clone(),
        control: inv.control.clone(),
    }
}

pub(crate) fn session_server(args: &[String]) -> Result<()> {
    let accepted = [
        "--home",
        "--parent-pid",
        "--parent-created",
        "--start-token",
    ];
    if args.len() != 9 || args[0] != "--source-native-session-server" {
        return Err(ToolkitError::arguments(
            "private native Source startup requires exact owned arguments",
        ));
    }
    let mut flags = std::collections::BTreeMap::new();
    for pair in args[1..].as_chunks::<2>().0 {
        if !accepted.contains(&pair[0].as_str())
            || pair[1].is_empty()
            || flags.insert(pair[0].as_str(), pair[1].as_str()).is_some()
        {
            return Err(ToolkitError::arguments(
                "invalid private native Source startup arguments",
            ));
        }
    }
    let parent_pid = flags["--parent-pid"]
        .parse::<u32>()
        .map_err(|_| ToolkitError::arguments("invalid native Source parent PID"))?;
    let parent_created = flags["--parent-created"]
        .parse::<u64>()
        .map_err(|_| ToolkitError::arguments("invalid native Source parent creation"))?;
    let home = std::path::PathBuf::from(flags["--home"]);
    if !home.is_absolute() {
        return Err(ToolkitError::arguments(
            "native Source home must be absolute",
        ));
    }
    lycheedev_source::native_session::server(lycheedev_source::native_session::SessionStartup {
        home,
        parent_pid,
        parent_created,
        start_token: flags["--start-token"].into(),
    })
}

pub(crate) enum Operation {
    Batch {
        request: batch::BatchRequest,
        flow: bool,
    },
    Refs(RelationRequest),
    Context(ContextRequest),
}

pub(crate) struct Research {
    selection: ResearchSelection,
    mode: ResearchMode,
    operation: Operation,
}

impl Research {
    pub(crate) fn needs_tools(&self) -> bool {
        match &self.operation {
            Operation::Batch { request, flow } => {
                request.needs_research() && (self.mode.uses_native() || *flow)
            }
            Operation::Refs(_) => self.mode.uses_native(),
            Operation::Context(request) => self.mode.uses_native() || request.flow,
        }
    }

    pub(crate) fn dispatch(self, ctx: &SourceContext, tools: SourceTools<'_>) -> Result<Outcome> {
        match self.operation {
            Operation::Batch { request, flow } => {
                batch::dispatch_request(ctx, &self.selection, request, self.mode, flow, tools)
            }
            Operation::Refs(request) => lycheedev_source::refs::dispatch_with_tools(
                ctx,
                &self.selection,
                &request,
                self.mode,
                tools,
            ),
            Operation::Context(request) => lycheedev_source::context::dispatch_with_tools(
                ctx,
                &self.selection,
                &request,
                self.mode,
                tools,
            ),
        }
    }

    pub(crate) fn unavailable(self, ctx: &SourceContext, error: ToolkitError) -> Result<Outcome> {
        match self.operation {
            Operation::Batch { request, flow } => {
                batch::dispatch_unavailable(ctx, &self.selection, request, flow, error)
            }
            _ => Err(error),
        }
    }
}

fn selection_arguments() -> ToolkitError {
    ToolkitError::arguments(
        "source refs/context require --snapshot and exactly one of --symbol-id or --symbol",
    )
}

fn selector(inv: &Invocation) -> Result<SymbolSelector> {
    match (inv.flag("--symbol-id"), inv.flag("--symbol")) {
        (Some(id), None) => Ok(SymbolSelector::Id(id.into())),
        (None, Some(name)) => Ok(SymbolSelector::Name(name.into())),
        _ => Err(selection_arguments()),
    }
}

fn mode(inv: &Invocation) -> Result<ResearchMode> {
    if inv.flag_bool("--static-only") {
        if inv.flag("--release").is_some() || inv.flag_bool("--session-reuse") {
            return Err(ToolkitError::arguments(
                "--static-only cannot be combined with --release or --session-reuse",
            ));
        }
        Ok(ResearchMode::Static)
    } else if inv.flag_bool("--session-reuse") {
        Ok(ResearchMode::Session)
    } else {
        Ok(ResearchMode::OneShot)
    }
}

fn research_pin(inv: &Invocation) -> Result<selection::PinnedSet> {
    if let Some((_, pin)) = crate::core_cli::resolve_optional_project(inv)? {
        return Ok(pin);
    }
    if inv.flag("--snapshot").is_none_or(str::is_empty) {
        return Err(selection_arguments());
    }
    crate::core_cli::resolve_pinned(inv)
}

pub(crate) fn research(inv: &Invocation) -> Result<Option<Research>> {
    let (pin, operation, mode, needs_environment) = match inv.command.as_str() {
        "source batch" => {
            let path = Path::new(inv.required("--file")?);
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                inv.cwd.join(path)
            };
            let request = batch::read(&path, &inv.control)?;
            let mode = mode(inv)?;
            let needs_environment = request.needs_research();
            let pin = crate::core_cli::resolve_pinned(inv)?;
            (
                pin,
                Operation::Batch {
                    request,
                    flow: inv.flag_bool("--flow"),
                },
                mode,
                needs_environment,
            )
        }
        "source refs" => {
            // A nearer project is selected before selector admission. An
            // explicit snapshot bypasses project discovery entirely.
            let project = crate::core_cli::resolve_optional_project(inv)?;
            if project.is_none() && inv.flag("--snapshot").is_none_or(str::is_empty) {
                return Err(selection_arguments());
            }
            let selector = selector(inv)?;
            let mode = mode(inv)?;
            if inv.flag_bool("--flow") {
                return Err(ToolkitError::arguments(
                    "--flow is evaluated by source context",
                ));
            }
            let limit = inv.usize_flag("--limit", 50, 1, 200)?;
            let direction = match inv.flag("--direction").unwrap_or("both") {
                "incoming" => RelationDirection::Incoming,
                "outgoing" => RelationDirection::Outgoing,
                "both" => RelationDirection::Both,
                _ => {
                    return Err(ToolkitError::arguments(
                        "--direction must be incoming, outgoing, or both",
                    ));
                }
            };
            let pin = match project {
                Some((_, pin)) => pin,
                None => crate::core_cli::resolve_pinned(inv)?,
            };
            (
                pin,
                Operation::Refs(RelationRequest {
                    selector,
                    direction,
                    limit,
                    cursor: inv.flag("--cursor").unwrap_or("").into(),
                }),
                mode,
                true,
            )
        }
        "source context" => {
            let limit = inv.usize_flag("--limit", 50, 1, 200)?;
            let max_lines = inv
                .usize_flag("--max-lines", 120, 1, 2000)
                .map_err(|_| ToolkitError::arguments("--max-lines must be between 1 and 2000"))?;
            let max_bytes = inv
                .usize_flag("--max-bytes", 16 << 10, 1, 1 << 20)
                .map_err(|_| {
                    ToolkitError::arguments(
                        "source context --max-bytes must be between 1 and 1048576",
                    )
                })?;
            let depth = inv
                .usize_flag("--depth", 1, 0, 4)
                .map_err(|_| ToolkitError::arguments("--depth must be between 0 and 4"))?;
            let pin = research_pin(inv)?;
            let selector = selector(inv)?;
            let mode = mode(inv)?;
            (
                pin,
                Operation::Context(ContextRequest {
                    selector,
                    limit,
                    max_bytes,
                    max_lines,
                    depth,
                    cursor: inv.flag("--cursor").unwrap_or("").into(),
                    flow: inv.flag_bool("--flow"),
                }),
                mode,
                true,
            )
        }
        _ => return Ok(None),
    };
    let environment = if needs_environment {
        inv.flag("--environment")
            .map(|id| selection::load_pinned(&inv.home, id))
            .transpose()?
    } else {
        None
    };
    Ok(Some(Research {
        selection: ResearchSelection { pin, environment },
        mode,
        operation,
    }))
}

pub(crate) enum SourceCommand {
    Research(Research),
    Ordinary(Ordinary),
    Session(lycheedev_source::native_session::SessionAction),
}

pub(crate) struct Ordinary {
    request: SourceRequest,
    project_context: Option<Value>,
}

impl SourceCommand {
    pub(crate) fn needs_tools(&self) -> bool {
        match self {
            Self::Research(request) => request.needs_tools(),
            Self::Ordinary(request) => matches!(
                request.request,
                SourceRequest::Validate(_) | SourceRequest::Matrix(_)
            ),
            Self::Session(_) => false,
        }
    }

    pub(crate) fn dispatch(self, ctx: &SourceContext, tools: SourceTools<'_>) -> Result<Outcome> {
        match self {
            Self::Research(request) => request.dispatch(ctx, tools),
            Self::Ordinary(request) => {
                let mut outcome =
                    lycheedev_source::dispatch_with_tools(ctx, &request.request, tools)
                        .map_err(|error| project_error(error, request.project_context.as_ref()))?;
                if let Some(context) = request.project_context {
                    outcome.selection = context;
                }
                Ok(outcome)
            }
            Self::Session(action) => lycheedev_source::native_session::dispatch(ctx, action),
        }
    }

    pub(crate) fn unavailable(self, ctx: &SourceContext, error: ToolkitError) -> Result<Outcome> {
        match self {
            Self::Research(request) => request.unavailable(ctx, error),
            _ => Err(error),
        }
    }
}

fn project_error(mut error: ToolkitError, context: Option<&Value>) -> ToolkitError {
    if let Some(context) = context {
        if !error.details.is_object() {
            error.details = if error.details.is_null() {
                json!({})
            } else {
                json!({"primaryDetails": error.details})
            };
        }
        error.details["__commandContext"] = context.clone();
    }
    error
}

fn inspect(inv: &Invocation) -> Result<Ordinary> {
    let project = crate::core_cli::resolve_optional_project(inv).map_err(|mut error| {
        if [
            "selection.project_missing",
            "selection.project_unlocked",
            "selection.project_format",
            "selection.project_conflict",
        ]
        .contains(&error.code.as_str())
        {
            if !error.details.is_object() {
                error.details = if error.details.is_null() {
                    json!({})
                } else {
                    json!({"primaryDetails": error.details})
                };
            }
            error.details["stage"] = json!("selection");
        }
        error
    })?;
    let project_context = project
        .as_ref()
        .map(|(project, pin)| json!({"project": project, "snapshot": pin.id}));
    let request = (|| {
        if inv.flag("--snapshot").is_none_or(str::is_empty) && project.is_none()
            || ["--path", "--symbol", "--target-path"]
                .iter()
                .filter(|flag| inv.flag(flag).is_some_and(|value| !value.is_empty()))
                .count()
                != 1
        {
            return Err(ToolkitError::arguments(
                "source sync requires --source/--product; source inspect requires --snapshot and exactly one of --path, --symbol, --target-path",
            ));
        }
        let pin = match project {
            Some((_, pin)) => pin,
            None => crate::core_cli::resolve_pinned(inv)?,
        };
        let selection = SelectedSource::new(pin)?;
        let target = if let Some(path) = inv.flag("--path").filter(|value| !value.is_empty()) {
            InspectTarget::Excerpt {
                path: path.into(),
                first_line: inv.usize_flag("--line", 1, 1, usize::MAX)?,
                line_count: inv.usize_flag("--count", 80, 1, 2000)?,
            }
        } else if let Some(symbol) = inv.flag("--symbol").filter(|value| !value.is_empty()) {
            InspectTarget::Symbol(symbol.into())
        } else {
            InspectTarget::TargetPath(inv.required("--target-path")?.into())
        };
        Ok(SourceRequest::Inspect { selection, target })
    })()
    .map_err(|error| project_error(error, project_context.as_ref()))?;
    Ok(Ordinary {
        request,
        project_context,
    })
}

fn absolute_input(inv: &Invocation, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        inv.cwd.join(path)
    }
}

fn validation(inv: &Invocation) -> Result<SourceRequest> {
    if let Some(path) = inv.flag("--matrix") {
        for flag in ["--snapshot", "--path", "--toc", "--environment"] {
            if inv.flag(flag).is_some() {
                return Err(ToolkitError::arguments(
                    "source validate requires either --matrix or --snapshot/--path/--toc; matrix targets require their own client source",
                ));
            }
        }
        let configuration = absolute_input(inv, path);
        let (config, root) = lycheedev_source::matrix::read_config(&configuration, &inv.control)?;
        return Ok(SourceRequest::Matrix(MatrixValidationRequest {
            configuration,
            root,
            targets: config.targets,
            semantic: inv.flag_bool("--semantic"),
        }));
    }
    let selection = SelectedSource::new(crate::core_cli::resolve_pinned(inv)?)?;
    let path = inv.required("--path")?;
    let root = absolute_input(inv, path);
    let semantic = inv.flag_bool("--semantic");
    let environment = if selection.source().repository != "wow-ui-source" && semantic {
        Some(
            selection::load_pinned(&inv.home, inv.required("--environment")?)?
                .spec
                .source
                .ok_or_else(|| {
                    ToolkitError::new(
                        "codebase.client_source_pin_required",
                        "Environment pin has no source",
                        3,
                    )
                })?,
        )
    } else {
        None
    };
    let toc = inv.required("--toc")?;
    Ok(SourceRequest::Validate(AddonValidationRequest {
        selection,
        root,
        toc: toc.into(),
        semantic,
        environment,
        locator: format!("{path}/{toc}"),
    }))
}

fn ordinary(inv: &Invocation) -> Result<Ordinary> {
    let request = match inv.command.as_str() {
        "source list" => match inv.flag("--snapshot") {
            Some(_) => SourceRequest::Status(SelectedSource::new(crate::core_cli::resolve_pinned(inv)?)?),
            None => SourceRequest::Catalog,
        },
        "source sync" => SourceRequest::Sync(SyncRequest {
            repository: inv.required("--source")?.into(),
            product: inv.required("--product")?.into(),
            reference: inv.flag("--ref").unwrap_or("").into(),
        }),
        "source inspect" => return inspect(inv),
        "source index" => {
            SourceRequest::Index(SelectedSource::new(crate::core_cli::resolve_pinned(inv)?)?)
        }
        "source query" => SourceRequest::Query {
            selection: SelectedSource::new(crate::core_cli::resolve_pinned(inv)?)?,
            request: QueryRequest {
                query: inv.argument()?.into(),
                mode: inv.flag("--mode").unwrap_or("").into(),
                topic: inv.flag("--topic").unwrap_or("").into(),
                limit: inv.usize_flag("--limit", 50, 1, 200)?,
                cursor: inv.flag("--cursor").unwrap_or("").into(),
            },
        },
        "source validate" => validation(inv)?,
        "source diff" => {
            let from = inv.required("--from")?;
            let to = inv.required("--to")?;
            let left = selection::load_pinned(&inv.home, from)?;
            let right = selection::load_pinned(&inv.home, to)?;
            let selected = |pin| {
                SelectedSource::new(pin).map_err(|_| {
                    ToolkitError::new(
                        "codebase.source_pin_required",
                        "codebase.source_pin_required",
                        5,
                    )
                })
            };
            SourceRequest::Diff(DiffRequest {
                from: selected(left)?,
                to: selected(right)?,
                limit: inv.usize_flag("--limit", 50, 1, 200)?,
            })
        }
        "source prune" => {
            let target_bytes = inv
                .required("--target-bytes")?
                .parse::<u64>()
                .ok()
                .filter(|target| *target <= i64::MAX as u64)
                .ok_or_else(|| {
                    ToolkitError::arguments(
                        "source prune --target-bytes requires a nonnegative signed 64-bit integer",
                    )
                })?;
            SourceRequest::Prune { target_bytes }
        }
        _ => {
            return Err(ToolkitError::new(
                "command.unsupported",
                format!("Unknown Source command: {}", inv.command),
                3,
            ));
        }
    };
    Ok(Ordinary {
        request,
        project_context: None,
    })
}

pub(crate) fn admit(inv: &Invocation) -> Result<SourceCommand> {
    inv.control.check()?;
    match inv.command.as_str() {
        "source session status" => {
            return Ok(SourceCommand::Session(
                lycheedev_source::native_session::SessionAction::Status,
            ));
        }
        "source session close" => {
            return Ok(SourceCommand::Session(
                lycheedev_source::native_session::SessionAction::Close,
            ));
        }
        _ => {}
    }
    if let Some(request) = research(inv)? {
        return Ok(SourceCommand::Research(request));
    }
    ordinary(inv).map(SourceCommand::Ordinary)
}

#[cfg(test)]
#[path = "source_cli_tests.rs"]
mod tests;
