//! Explicit Wago and Raidbots providers from fixed Go v3.2.3. No fallback source.
use crate::{
    capture, fault,
    resource::{Budget, Cost},
};
use lycheedev_core::{
    Invocation, Outcome, Result, ToolkitError, control::OperationControl, evidence, selection,
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
pub(crate) use model::{equal_fold, lower as identifier_lower};
/// The command parser's admitted Hotfix option check, shared by all three sources.
pub fn validate_option(key: &str, value: &str) -> Result<()> {
    model::validate_option(key, value)
}
fn error(kind: &str, exit: i32, message: impl Into<String>) -> ToolkitError {
    let code = format!("records.hotfix_{kind}");
    let message = message.into();
    fault(&code, exit, format!("{code}: {message}"))
}
fn limit() -> ToolkitError {
    error("budget", 3, "hotfix result exceeds its resource budget")
}
fn go_bytes(value: &impl Serialize, budget: &Budget, maximum: usize) -> Result<Vec<u8>> {
    capture::json_bytes_with_formatter(value, budget, maximum, limit, HotfixFormatter)
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
        serde_json::ser::Formatter::write_string_fragment(&mut capture::GoCaptureFormatter, w, s)
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
fn budget(inv: &Invocation) -> Budget {
    let mut limits = Cost::defaults();
    if inv.flag("--source") == Some("raidbots") {
        limits.retained_bytes += 512 << 20;
    }
    Budget::controlled(limits, inv.control.deadline(), inv.control.clone())
}
#[cfg(test)]
fn pin(inv: &Invocation, change: selection::ChangePin, budget: &Budget) -> Result<String> {
    check_scope(inv, budget)?;
    // Go PinHotfixChange enters WriteMetadata before comparing the parent or
    // marshaling the new selection identity. Keep that observable precedence.
    let owner = lycheedev_core::metadata::Metadata::open_controlled_with_check(
        &inv.home,
        true,
        &inv.control,
        || budget.check(),
    )?;
    let result = pin_opened(inv, change, budget);
    close_metadata(owner, result, inv, budget)
}
fn pin_opened(inv: &Invocation, change: selection::ChangePin, budget: &Budget) -> Result<String> {
    inv.control.check()?;
    let parent = inv.flag("--snapshot").unwrap_or_default();
    if !parent.is_empty() {
        let existing =
            selection::load_pinned_controlled_with_check(&inv.home, parent, &inv.control, || {
                budget.check()
            })?;
        inv.control.check()?;
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
        &inv.home,
        selection::SelectionSpec {
            parent: parent.into(),
            changes: Some(change),
            ..Default::default()
        },
        &inv.control,
        || budget.check(),
    )?
    .id)
}
fn run(inv: &Invocation, budget: &Budget) -> Result<RemoteResult> {
    if inv.flag_bool("--scan") {
        return Err(ToolkitError::arguments("--scan requires --source dbcache"));
    }
    let query = Query::from_invocation(inv)?;
    inv.control.check()?;
    lycheedev_core::workspace::open_controlled_with_check(&inv.home, &inv.control, || {
        budget.check()
    })?;
    let mut result = match inv.required("--source")? {
        "raidbots" => snapshot::query(inv, query, budget)?,
        "wago" => wago::query(
            inv,
            query,
            FetchBudget {
                max_requests: inv
                    .flag("--max-requests")
                    .map(str::parse)
                    .transpose()
                    .map_err(|_| ToolkitError::arguments("--max-requests must be at least 1"))?
                    .unwrap_or(0),
                // Go entry.go supplies 128 MiB. Provider zero values independently default to 256 MiB.
                max_bytes: inv
                    .flag("--max-bytes")
                    .map(str::parse)
                    .transpose()
                    .map_err(|_| {
                        ToolkitError::arguments("--max-bytes must be between 1 and 536870912")
                    })?
                    .unwrap_or(128 << 20),
                ..Default::default()
            },
            budget,
            &wago::PublicFetcher,
            "https://wago.tools",
        )?,
        _ => {
            return Err(ToolkitError::arguments(
                "--source must be wago, dbcache, or raidbots",
            ));
        }
    };
    // Fixed QueryWagoHotfix holds WriteMetadata through pin, result marshal and
    // capture. Raidbots uses the same owner through its pin before presentation.
    let owner = lycheedev_core::metadata::Metadata::open_controlled_with_check(
        &inv.home,
        true,
        &inv.control,
        || budget.check(),
    )?;
    let published: Result<()> = (|| {
        result.snapshot = pin_opened(inv, result.change.clone(), budget)?;
        if result.change.provider == "wago" {
            result.query.check_time_marshal()?;
            let raw = go_bytes(&result, budget, 32 << 20)?;
            result.capture = Some(evidence::commit_capture_budgeted(
                &inv.home,
                evidence::CaptureInput {
                    bytes: &raw,
                    maximum_bytes: 32 << 20,
                    media_type: "application/json",
                    provenance: json!({"kind":"hotfix-records","locator":format!("{}:sha256:{}",result.change.provider,result.change.sha256),"snapshot":result.snapshot,"dataBuild":result.query.full_build}),
                    complete: result.complete,
                    truncated: result.truncated,
                },
                &inv.control,
                budget,
            )?);
        }
        Ok(())
    })();
    committed_result(close_metadata(owner, published, inv, budget), &result)?;
    Ok(result)
}
fn outcome(result: &RemoteResult, data: Value) -> Outcome {
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
pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    let budget = budget(inv);
    let result = run(inv, &budget)?;
    let presented = (|| {
        check_scope(inv, &budget)?;
        let data = if inv.flag("--encoding") == Some("csv") {
            if inv.flag("--output").unwrap_or_default().is_empty() {
                return Err(ToolkitError::arguments(
                    "data hotfix --encoding csv requires --output",
                ));
            }
            output::csv(inv, &result, &budget)?
        } else {
            serde_json::to_value(&result)?
        };
        finish_scope(Ok(outcome(&result, data)), inv, &budget)
    })();
    committed_result(presented, &result)
}
/// The CLI's special route emits the fixed typed record stream, not a generic envelope.
pub fn stream(inv: &Invocation, emit: &mut impl FnMut(&[u8]) -> Result<()>) -> Result<Outcome> {
    let budget = budget(inv);
    let result = run(inv, &budget)?;
    let presented = output::jsonl(&result, &inv.control, &budget, emit).map_err(output_failure);
    committed_result(presented, &result)?;
    Ok(outcome(&result, Value::Null))
}

/// Preserve provider RawMessage values through the final public presentation.
/// Only this command uses this narrow route; CSV remains a normal manifest outcome.
pub fn render(
    inv: &Invocation,
    timeout: Option<u64>,
    out: &mut impl std::io::Write,
) -> Result<Outcome> {
    let budget = budget(inv);
    let result = run(inv, &budget)?;
    let presented = output::render(inv, &result, timeout, out, &budget).map_err(output_failure);
    committed_result(presented, &result)?;
    Ok(outcome(&result, Value::Null))
}

fn check_scope(inv: &Invocation, budget: &Budget) -> Result<()> {
    inv.control.check()?;
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
    lycheedev_core::metadata::historical_write::finish_historical(result, stop)
}
fn finish_scope<T>(result: Result<T>, inv: &Invocation, budget: &Budget) -> Result<T> {
    finish_stop(finish_stop(result, inv.control.check()), budget.check())
}
fn close_metadata<T>(
    metadata: lycheedev_core::metadata::Metadata,
    result: Result<T>,
    inv: &Invocation,
    budget: &Budget,
) -> Result<T> {
    finish_scope(
        lycheedev_core::metadata::historical_write::finish_historical(result, metadata.close()),
        inv,
        budget,
    )
}
#[cfg(test)]
mod control_tests;

fn output_failure(original: ToolkitError) -> ToolkitError {
    fault("command.output_failed", 5, original.message.clone())
        .with_details(json!({"originalCause":original}))
}
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
