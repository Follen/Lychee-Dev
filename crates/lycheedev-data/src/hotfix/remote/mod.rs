//! Explicit Wago and Raidbots providers from fixed Go v3.2.3. No fallback source.
use crate::{
    capture, fault,
    resource::{Budget, Cost},
};
use lycheedev_core::{
    Outcome, Result, ToolkitError, control::OperationControl, evidence, selection,
    storage,
};
use serde::Serialize;
use serde_json::{Value, json};
mod decode;
mod entities;
mod model;
mod output;
mod parse;
mod snapshot;
#[cfg(test)]
mod tests;
mod wago;
use model::*;
use crate::request::DataContext;
use std::path::PathBuf;
pub use model::{Query, Record, FetchBudget, Coverage, RemoteResult};

#[derive(Clone, Debug)]
pub enum Provider { Wago, Raidbots(PathBuf) }
#[derive(Clone, Debug)]
pub struct Request {
    pub provider: Provider,
    pub query: Query,
    pub max_pages: i64,
    pub fetch: FetchBudget,
    pub csv: Option<PathBuf>,
}

pub fn normalize_time(value: &str) -> Result<String> { model::time_wire(value) }
fn error(kind: &str, exit: i32, message: impl Into<String>) -> ToolkitError {
    let code = format!("records.hotfix_{kind}");
    let message = message.into();
    fault(&code, exit, format!("{code}: {message}"))
}
fn limit() -> ToolkitError {
    error("budget", 3, "hotfix result exceeds its resource budget")
}
// Go json.Marshal compacts RawMessage while retaining duplicate keys and tokens.
// The shared string formatter supplies the same HTML/JavaScript escaping.
struct HotfixFormatter;
impl serde_json::ser::Formatter for HotfixFormatter {
    fn write_string_fragment<W: ?Sized + std::io::Write>(
        &mut self,
        w: &mut W,
        s: &str,
    ) -> std::io::Result<()> {
        serde_json::ser::Formatter::write_string_fragment(&mut capture::EvidenceFormatter, w, s)
    }
    fn write_raw_fragment<W: ?Sized + std::io::Write>(
        &mut self,
        w: &mut W,
        raw: &str,
    ) -> std::io::Result<()> {
        let mut string = false;
        let mut escaped = false;
        for ch in raw.chars() {
            if !string && matches!(ch, ' ' | '\n' | '\r' | '\t') {
                continue;
            }
            if ch == '"' && !escaped {
                string = !string;
            }
            let replacement: Option<&[u8]> = match ch {
                '<' => Some(b"\\u003c"),
                '>' => Some(b"\\u003e"),
                '&' => Some(b"\\u0026"),
                '\u{2028}' => Some(b"\\u2028"),
                '\u{2029}' => Some(b"\\u2029"),
                _ => None,
            };
            if let Some(bytes) = replacement {
                w.write_all(bytes)?;
            } else {
                w.write_all(ch.encode_utf8(&mut [0; 4]).as_bytes())?;
            }
            escaped = string && ch == '\\' && !escaped;
        }
        Ok(())
    }
}
fn budget(ctx: &DataContext, request: &Request) -> Budget {
    let mut limits = Cost::defaults();
    if matches!(&request.provider, Provider::Raidbots(_)) {
        limits.retained_bytes += 512 << 20;
    }
    Budget::controlled(limits, ctx.control.deadline(), ctx.control.clone())
}
#[cfg(test)]
fn pin(ctx: &DataContext, change: selection::ChangePin, budget: &Budget) -> Result<String> {
    check_scope(ctx, budget)?;
    // Go PinHotfixChange enters WriteMetadata before comparing the parent or
    // marshaling the new selection identity. Keep that observable precedence.
    let owner = lycheedev_core::metadata::Metadata::open_controlled_with_check(
        &ctx.home,
        true,
        &ctx.control,
        || budget.check(),
    )?;
    let result = pin_opened(ctx, change, budget);
    close_metadata(owner, result, ctx, budget)
}
fn pin_opened(ctx: &DataContext, change: selection::ChangePin, budget: &Budget) -> Result<String> {
    ctx.control.check()?;
    let parent = ctx.source.snapshot.as_deref().unwrap_or_default();
    if !parent.is_empty() {
        let existing =
            selection::load_pinned_controlled_with_check(&ctx.home, parent, &ctx.control, || {
                budget.check()
            })?;
        ctx.control.check()?;
        if let Some(pinned) = &existing.changes {
            if *pinned != change {
                return Err(error(
                    "identity",
                    4,
                    "parent already pins a different change",
                ));
            }
            return Ok(existing.id);
        }
    }
    Ok(selection::pin_selection_controlled_with_check(
        &ctx.home,
        selection::SelectionSpec {
            parent: parent.into(),
            changes: Some(change),
            ..Default::default()
        },
        &ctx.control,
        || budget.check(),
    )?
    .id)
}
fn run(ctx: &DataContext, request: &Request, budget: &Budget) -> Result<RemoteResult> {
    let query = request.query.clone();
    query.validate()?;
    ctx.control.check()?;
    lycheedev_core::workspace::open_controlled_with_check(&ctx.home, &ctx.control, || budget.check())?;
    let mut result = match &request.provider {
        Provider::Raidbots(path) => snapshot::query(ctx, path, query, budget)?,
        Provider::Wago => wago::query(ctx, query, request.fetch.clone(), request.max_pages, budget, &wago::PublicFetcher, "https://wago.tools")?,
    };
    // Fixed QueryWagoHotfix holds WriteMetadata through pin, result marshal and
    // capture. Raidbots uses the same owner through its pin before presentation.
    let owner = lycheedev_core::metadata::Metadata::open_controlled_with_check(
        &ctx.home,
        true,
        &ctx.control,
        || budget.check(),
    )?;
    let published: Result<()> = (|| {
        result.snapshot = pin_opened(ctx, result.change.clone(), budget)?;
        if result.change.provider == "wago" {
            result.query.check_time_marshal()?;
            let raw = wire_bytes(&result, budget, 32 << 20)?;
            result.capture = Some(evidence::commit_capture_budgeted(
                &ctx.home,
                evidence::CaptureInput {
                    bytes: &raw,
                    maximum_bytes: 32 << 20,
                    media_type: "application/json",
                    provenance: json!({"kind":"hotfix-records","locator":format!("{}:sha256:{}",result.change.provider,result.change.sha256),"snapshot":result.snapshot,"dataBuild":result.query.full_build}),
                    complete: result.complete,
                    truncated: result.truncated,
                },
                &ctx.control,
                budget,
            )?);
        }
        Ok(())
    })();
    committed_result(close_metadata(owner, published, ctx, budget), &result)?;
    Ok(result)
}
pub fn outcome(result: &RemoteResult, data: Value) -> Outcome {
    let mut out = Outcome::new(data);
    out.complete = result.complete;
    out.selection = json!({"snapshot":result.snapshot});
    out.warnings = result.warnings.iter().map(|w| json!(w)).collect();
    if !result.next_cursor.is_empty() {
        out.warnings.push(json!("This page is not the complete record set; preserve the cursor and query scope when continuing."));
    }
    if let Some(capture) = &result.capture {
        out.provenance.push(capture.clone());
    }
    out
}
/// Keep the committed result and its live budget owned through presentation.
pub fn with_result(ctx: &DataContext, request: &Request,
    present: impl FnOnce(&RemoteResult, &Budget) -> Result<Outcome>) -> Result<Outcome> {
    let budget = budget(ctx, request);
    let result = run(ctx, request, &budget)?;
    let presented = (|| { check_scope(ctx, &budget)?; finish_scope(present(&result, &budget), ctx, &budget) })();
    committed_result(presented, &result)
}
pub fn dispatch(ctx: &DataContext, request: &Request) -> Result<Outcome> {
    with_result(ctx, request, |result, budget| {
        let data = match &request.csv {
            Some(path) => output::csv(ctx, path, result, budget)?,
            None => serde_json::to_value(result)?,
        };
        Ok(outcome(result, data))
    })
}
pub fn wire_bytes(value: &impl Serialize, budget: &Budget, maximum: usize) -> Result<Vec<u8>> {
    capture::json_bytes_with_formatter(value, budget, maximum, limit, HotfixFormatter)
}

fn check_scope(ctx: &DataContext, budget: &Budget) -> Result<()> {
    ctx.control.check()?;
    budget.check()
}
pub(super) fn finish_stop<T>(result: Result<T>, stop: Result<()>) -> Result<T> {
    if let (Err(a), Err(b)) = (&result, &stop) {
        if a.code == b.code
            && a.exit_code == b.exit_code
            && a.message == b.message
            && a.details == b.details
        {
            return result;
        }
    }
    lycheedev_core::metadata::owned_write::finish_owned(result, stop)
}
fn finish_scope<T>(result: Result<T>, ctx: &DataContext, budget: &Budget) -> Result<T> {
    finish_stop(finish_stop(result, ctx.control.check()), budget.check())
}
fn close_metadata<T>(
    metadata: lycheedev_core::metadata::Metadata,
    result: Result<T>,
    ctx: &DataContext,
    budget: &Budget,
) -> Result<T> {
    finish_scope(
        lycheedev_core::metadata::owned_write::finish_owned(result, metadata.close()),
        ctx,
        budget,
    )
}
#[cfg(test)]
mod control_tests;

fn committed_result<T>(result: Result<T>, reading: &RemoteResult) -> Result<T> {
    result.map_err(|mut error| {
        if let Some(id) = reading.capture.as_ref().and_then(|capture| capture["id"].as_str()) {
            let primary = error.clone();
            error.details = json!({"schema":"lycheedev.stream-committed-error.v1", "primary":primary, "completedCaptures":[id]});
            error.message.push_str(&format!("\nhotfix capture already committed: {id}"));
        }
        error
    })
}
