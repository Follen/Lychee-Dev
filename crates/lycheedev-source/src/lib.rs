#![forbid(unsafe_code)]
//! Immutable source research; never evaluates indexed repository programs.
mod assets;
pub mod batch;
pub mod catalog;
pub mod closure;
pub mod context;
pub mod diff;
mod encoding;
mod engine_config;
pub mod environment;
pub mod facts;
pub mod flow;
pub mod git;
pub mod index;
mod inspect;
pub mod lua_tree;
pub mod luac;
pub mod matrix;
pub mod native;
pub mod native_research;
pub mod native_semantic;
pub mod native_session;
mod persistence;
mod process;
pub mod prune;
mod query_store;
pub mod refs;
pub mod relations;
pub mod request;
mod research_material;
pub mod validate;
pub mod xml_inline;
pub mod xml_load;
pub mod xml_program;

use lycheedev_core::{Outcome, Result, ToolkitError, evidence, selection};
use request::{
    AddonValidationRequest, InspectTarget, QueryRequest, SelectedSource, SourceContext,
    SourceRequest,
};
use serde_json::json;

#[derive(Default)]
pub struct SourceTools<'a> {
    pub luac: Option<&'a luac::Runtime>,
}

pub fn dispatch(ctx: &SourceContext, request: &SourceRequest) -> Result<Outcome> {
    dispatch_with_tools(ctx, request, SourceTools::default())
}

pub fn dispatch_with_tools(
    ctx: &SourceContext,
    request: &SourceRequest,
    tools: SourceTools<'_>,
) -> Result<Outcome> {
    ctx.control.check()?;
    match request {
        SourceRequest::Catalog => Ok(Outcome::new(serde_json::to_value(catalog::repositories())?)),
        SourceRequest::Status(selection) => status(ctx, selection),
        SourceRequest::Sync(request) => sync(ctx, request),
        SourceRequest::Inspect { selection, target } => inspect(ctx, selection, target),
        SourceRequest::Index(selection) => index(ctx, selection),
        SourceRequest::Query { selection, request } => query(ctx, selection, request),
        SourceRequest::Validate(request) => validate(ctx, request, &tools),
        SourceRequest::Matrix(request) => matrix::dispatch(ctx, request, tools),
        SourceRequest::Diff(request) => diff::dispatch(ctx, request),
        SourceRequest::Prune { target_bytes } => prune::dispatch(ctx, *target_bytes),
    }
}

fn status(ctx: &SourceContext, selected: &SelectedSource) -> Result<Outcome> {
    let mut out = Outcome::new(query_store::status_with_control(
        &ctx.home,
        selected.source(),
        &ctx.control,
    )?);
    out.coverage = out.data["coverage"].clone();
    out.selection = serde_json::to_value(selected.pin())?;
    Ok(out)
}

fn sync(ctx: &SourceContext, request: &request::SyncRequest) -> Result<Outcome> {
    let source = git::resolve_source_with_control(
        &ctx.home,
        &request.repository,
        &request.product,
        &request.reference,
        &ctx.control,
    )?;
    let pin = selection::pin_selection(
        &ctx.home,
        selection::SelectionSpec {
            source: Some(source),
            ..Default::default()
        },
    )?;
    let mut out = Outcome::new(serde_json::to_value(&pin)?);
    out.selection = serde_json::to_value(pin)?;
    Ok(out)
}

fn inspect(
    ctx: &SourceContext,
    selected: &SelectedSource,
    target: &InspectTarget,
) -> Result<Outcome> {
    let source = selected.source();
    let mut out = match target {
        InspectTarget::Excerpt {
            path,
            first_line,
            line_count,
        } => {
            let (raw, _) = git::blob_with_control(
                &ctx.home,
                source,
                path,
                git::MAX_SOURCE_BYTES,
                &ctx.control,
            )?;
            if raw.contains(&0) {
                return Err(ToolkitError::new(
                    "codebase.source_integrity",
                    "Source contains NUL",
                    4,
                ));
            }
            let mut starts = if raw.is_empty() { Vec::new() } else { vec![0] };
            for (i, byte) in raw.iter().enumerate() {
                if *byte == b'\n' && i + 1 < raw.len() {
                    starts.push(i + 1);
                }
            }
            if *first_line > starts.len() {
                return Err(ToolkitError::new(
                    "codebase.line_out_of_range",
                    "Requested line exceeds source",
                    2,
                ));
            }
            let last = starts
                .len()
                .min((*first_line).saturating_add(line_count - 1));
            let end = starts.get(last).copied().unwrap_or(raw.len());
            // Display conversion only: the archived blob and line coordinates
            // remain the original bytes.
            let text = encoding::display(&raw[starts[first_line - 1]..end]);
            let blob = evidence::publish_blob(&ctx.home, &raw, git::MAX_SOURCE_BYTES)?;
            let mut data = json!({
                "repository": source.repository,
                "commit": source.exact_commit,
                "path": path,
                "firstLine": first_line,
                "lastLine": last,
                "totalLines": starts.len(),
                "text": text,
                "blob": blob
            });
            if let Some(warning) = encoding::warning(&raw) {
                data["warnings"] = json!([warning.json(path)]);
            }
            Outcome::new(data)
        }
        InspectTarget::Symbol(symbol) => {
            let mut out = Outcome::new(inspect::target_with_control(
                &ctx.home,
                source,
                selected.snapshot(),
                Some(symbol),
                None,
                &ctx.control,
            )?);
            out.coverage = out.data["coverage"].clone();
            out.complete = out.data["complete"] == true;
            out
        }
        InspectTarget::TargetPath(path) => {
            let mut out = Outcome::new(inspect::target_with_control(
                &ctx.home,
                source,
                selected.snapshot(),
                None,
                Some(path),
                &ctx.control,
            )?);
            out.coverage = out.data["coverage"].clone();
            out.complete = out.data["complete"] == true;
            out
        }
    };
    let kind = match target {
        InspectTarget::Excerpt { .. } => "source-excerpt",
        InspectTarget::Symbol(_) | InspectTarget::TargetPath(_) => "source-inspect",
    };
    let capture = evidence::commit_capture_bounded(
        &ctx.home,
        evidence::CaptureInput {
            bytes: &serde_json::to_vec(&out.data)?,
            maximum_bytes: evidence::DEFAULT_CAPTURE_BYTES,
            media_type: "application/json",
            provenance: json!({"kind":kind,"locator":target.locator(),"snapshot":selected.snapshot(),"sourceCommit":source.exact_commit}),
            complete: out.complete,
            truncated: out.data["truncated"] == true,
        },
        &ctx.control,
    )?;
    out.provenance.push(capture);
    out.selection = serde_json::to_value(selected.pin())?;
    Ok(out)
}

fn index(ctx: &SourceContext, selected: &SelectedSource) -> Result<Outcome> {
    let (summary, coverage) =
        index::build_summary_with_control(&ctx.home, selected.source(), &ctx.control)?;
    let mut out = Outcome::new(summary);
    out.coverage = if out.data.get("coverage").is_some() {
        out.data["coverage"].clone()
    } else {
        coverage
    };
    out.complete = out.data["complete"] == true;
    add_source_capture(
        ctx,
        selected,
        &mut out,
        "source-query",
        selected.source().repository.as_str(),
    )?;
    Ok(out)
}

fn query(
    ctx: &SourceContext,
    selected: &SelectedSource,
    request: &QueryRequest,
) -> Result<Outcome> {
    let source = selected.source();
    let index = query_store::load_with_control(
        &ctx.home,
        source,
        &request.query,
        &request.mode,
        &request.topic,
        &ctx.control,
    )?;
    let coverage = index.coverage();
    let data = index.search_with_control(
        source,
        selected.snapshot(),
        &request.query,
        &request.mode,
        &request.topic,
        request.limit,
        &request.cursor,
        &ctx.control,
    )?;
    let mut out = Outcome::new(data);
    out.coverage = if out.data.get("coverage").is_some() {
        out.data["coverage"].clone()
    } else {
        coverage
    };
    out.complete = out.data["complete"] == true;
    add_source_capture(ctx, selected, &mut out, "source-query", &request.query)?;
    Ok(out)
}

fn add_source_capture(
    ctx: &SourceContext,
    selected: &SelectedSource,
    out: &mut Outcome,
    kind: &str,
    locator: &str,
) -> Result<()> {
    let capture = evidence::commit_capture_bounded(
        &ctx.home,
        evidence::CaptureInput {
            bytes: &serde_json::to_vec(&out.data)?,
            maximum_bytes: evidence::DEFAULT_CAPTURE_BYTES,
            media_type: "application/json",
            provenance: json!({"kind":kind,"locator":locator,"snapshot":selected.snapshot(),"sourceCommit":selected.source().exact_commit}),
            complete: out.complete,
            truncated: out.data["truncated"] == true,
        },
        &ctx.control,
    )?;
    out.provenance.push(capture);
    out.selection = serde_json::to_value(selected.pin())?;
    Ok(())
}

fn validate(
    ctx: &SourceContext,
    request: &AddonValidationRequest,
    tools: &SourceTools<'_>,
) -> Result<Outcome> {
    let selected = &request.selection;
    let source = selected.source();
    let data = validate::addon_with_environment(
        &ctx.home,
        source,
        selected.snapshot(),
        &request.root,
        &request.toc,
        tools,
        request.semantic,
        &ctx.control,
        request.environment.as_ref(),
    )?;
    let mut out = Outcome::new(data);
    out.complete = out.data["complete"] == true;
    let capture = evidence::commit_capture_bounded(
        &ctx.home,
        evidence::CaptureInput {
            bytes: &serde_json::to_vec(&out.data)?,
            maximum_bytes: evidence::DEFAULT_CAPTURE_BYTES,
            media_type: "application/json",
            provenance: json!({"kind":"addon-static-check","locator":request.locator,"snapshot":selected.snapshot(),"sourceCommit":source.exact_commit}),
            complete: out.complete,
            truncated: false,
        },
        &ctx.control,
    )?;
    out.provenance.push(capture);
    out.selection = serde_json::to_value(selected.pin())?;
    if out.data["staticValid"] == false {
        return Err(ToolkitError::new(
            "codebase.addon_static_check_failed",
            "Addon validation found invalid source",
            4,
        )
        .with_details(json!({"result":out.data,"captures":out.provenance,"context":{"selection":out.selection,"coverage":out.coverage,"snapshot":selected.snapshot()}})));
    }
    if !out.complete {
        out.warnings.push(json!(
            "Static coverage is incomplete; inspect unresolved references and notChecked."
        ));
    }
    Ok(out)
}
