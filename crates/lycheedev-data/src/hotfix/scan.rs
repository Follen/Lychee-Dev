//! Retained hotfix-scan wire contract: immutable pages, cumulative coverage and a
//! durable checkpoint after every page. A failed continuation returns only the
//! last published checkpoint, never an uncommitted in-memory advance.
use super::{CachePage, CacheFilter, LocalInput, LocalRequest, ScanRequest, local};
use crate::request::DataContext;
use crate::{
    capture, fault,
    resource::{Budget, Cost, RetainedReservation},
};
use lycheedev_core::{Outcome, Result, ToolkitError, evidence};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cell::RefCell;

const MANIFEST_BYTES: usize = 4 << 20;
const MAX_PAGES: usize = 10_000;
const WARNING: &str = "Scan coverage applies only to the selected immutable cache and filters, not server coverage. Page captures contain the records; resume with the returned capture when incomplete.";

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
struct Request {
    file: String,
    from: String,
    table: String,
    offline: bool,
    filter: RequestFilter,
    max_bytes: i64,
}
// Go's request Limit is a signed platform int (64-bit on the shipped
// Windows amd64 target). Keep signed wire values throughout repeated-field
// decoding, then compare against the independently validated current request.
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RequestFilter {
    build: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    table_hash: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    record_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    push: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    region: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<u8>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[serde(default)]
    latest: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    after_index: Option<u32>,
    limit: i64,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct State {
    snapshot: String,
    source: Option<evidence::CaptureRef>,
    query: Request,
    pages: Vec<String>,
    matched: u32,
    returned: u32,
    decoded: u32,
    no_payload: u32,
    not_valid: u32,
    raw: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_index: Option<u32>,
    complete: bool,
    truncated: bool,
}
#[derive(Deserialize)]
struct PageResult {
    snapshot: String,
    source: evidence::CaptureRef,
    page: CachePage,
}
fn identity(message: &str) -> ToolkitError {
    fault("records.hotfix_identity", 4, message)
}
fn limit() -> ToolkitError {
    fault("records.hotfix_limit", 3, "Hotfix scan exceeds its bound")
}
fn cursor() -> ToolkitError {
    fault(
        "records.hotfix_cursor",
        2,
        "Hotfix scan cursor did not advance consistently",
    )
}
fn ledger(ctx: &DataContext) -> Budget {
    // Each page and checkpoint has its own bounded live allocation lifetime,
    // just as Go's page loop. All share the original control and deadline.
    Budget::controlled(
        Cost {
            // One scan checkpoint may own 10,000 pages plus its source; core
            // metadata documents are bounded at 1 MiB each. These are bounded
            // cumulative I/O/work allowances, not live memory allowances.
            metadata_bytes: ((MAX_PAGES + 1) as u64 * (1 << 20)) + (128 << 20),
            decode_work: ((MAX_PAGES + 1) as u64 * (8 << 20)) + (256 << 20),
            ..Cost::defaults()
        },
        ctx.control.deadline(),
        ctx.control.clone(),
    )
}
// Dependencies are checked sequentially in the SAME metadata transaction.
// Keep the current read/parse/identity reservation until the next admission,
// when the preceding raw document and parsed objects have all been dropped.
// Default capture accounting remains unchanged for every other producer.
struct ScanCaptureAccounting<'a> {
    budget: &'a Budget,
    dependency: RefCell<Option<RetainedReservation<'a>>>,
}
impl<'a> ScanCaptureAccounting<'a> {
    fn new(budget: &'a Budget) -> Self {
        Self {
            budget,
            dependency: RefCell::new(None),
        }
    }
}
impl evidence::CaptureAccounting for ScanCaptureAccounting<'_> {
    fn evidence_domain(&self) -> lycheedev_core::metadata::EvidenceDomain {
        lycheedev_core::metadata::EvidenceDomain::Data
    }

    fn check(&self) -> Result<()> {
        self.budget.check()
    }
    fn admit_allocation(&self, bytes: usize) -> Result<()> {
        self.budget.charge(Cost::metadata(bytes))
    }
    fn admit_work(&self, units: usize) -> Result<()> {
        self.budget.charge(Cost::work(units))
    }
    fn admit_dependency(&self, bytes: usize) -> Result<()> {
        self.dependency.borrow_mut().take();
        self.budget.charge(Cost {
            metadata_bytes: bytes as u64,
            decode_work: bytes.saturating_mul(8) as u64,
            ..Default::default()
        })?;
        let reservation = self
            .budget
            .reserve_retained(bytes.saturating_mul(24).saturating_add(4096))?;
        *self.dependency.borrow_mut() = Some(reservation);
        Ok(())
    }
}
fn admit_page(state: &State) -> Result<()> {
    if state.pages.len() >= MAX_PAGES {
        return Err(limit());
    }
    Ok(())
}
fn request(page: &LocalRequest) -> Request {
    let (file, from) = match &page.input {
        LocalInput::File(path) => (path.to_string_lossy().into_owned(), String::new()),
        LocalInput::Capture(id) => (String::new(), id.clone()),
    };
    Request { file, from, table: page.table.clone().unwrap_or_default(), offline: page.offline,
        max_bytes: page.max_bytes as i64, filter: RequestFilter { build: 0, table_hash: page.filter.table_hash,
        record_id: page.filter.record_id, push: page.filter.push, region: page.filter.region, status: page.filter.status,
        latest: page.filter.latest, after_index: page.filter.after_index, limit: page.filter.limit as i64 } }
}
fn same_request(saved: &Request, current: &Request) -> bool {
    let mut current = current.clone();
    current.offline = saved.offline;
    saved == &current
}
fn page_request(state: &State, offline: bool) -> LocalRequest {
    LocalRequest { snapshot: state.snapshot.clone(),
        input: if state.query.file.is_empty() { LocalInput::Capture(state.query.from.clone()) }
            else { LocalInput::File(state.query.file.clone().into()) },
        table: (!state.query.table.is_empty()).then(|| state.query.table.clone()), offline,
        max_bytes: state.query.max_bytes as usize,
        filter: CacheFilter { build: 0, table_hash: state.query.filter.table_hash, record_id: state.query.filter.record_id,
            push: state.query.filter.push, region: state.query.filter.region, status: state.query.filter.status,
            latest: state.query.filter.latest, after_index: state.next_index, limit: state.query.filter.limit as usize } }
}
fn encoded_state(state: &State, budget: &Budget) -> Result<(Vec<u8>, Value)> {
    let encoded = capture::json_bytes(state, budget, MANIFEST_BYTES, limit)?;
    budget.charge(Cost::retained(
        encoded.len().saturating_mul(32).saturating_add(4096),
    ))?;
    let public = serde_json::from_slice(&encoded)?;
    budget.check()?;
    Ok((encoded, public))
}
fn outcome(state: Value, reference: Value, resume: bool) -> Outcome {
    let snapshot = state["snapshot"].clone();
    let source = state["source"].clone();
    let complete = state["complete"] == true;
    let mut result = json!({"result":state, "capture":reference});
    if resume {
        result["resume"] = result["capture"]["id"].clone();
    }
    let mut out = Outcome::new(result);
    out.complete = complete;
    out.selection = json!({"snapshot":snapshot});
    out.provenance = vec![source, out.data["capture"].clone()];
    out
}
fn checkpoint_error(mut error: ToolkitError, checkpoint: Option<Outcome>) -> ToolkitError {
    if let Some(checkpoint) = checkpoint {
        if !error.details.is_object() {
            error.details = json!({});
        }
        error.details["result"] = checkpoint.data;
        error.details["captures"] = Value::Array(checkpoint.provenance);
        error.details["__commandContext"] = checkpoint.selection;
    }
    error
}
pub(super) fn dispatch(ctx: &DataContext, request: &ScanRequest) -> Result<Outcome> {
    dispatch_with(ctx, request, local::dispatch)
}
fn dispatch_with(ctx: &DataContext, current: &ScanRequest,
    mut inspect: impl FnMut(&DataContext, &LocalRequest) -> Result<Outcome>) -> Result<Outcome> {
    ctx.control.check()?;
    let request = request(&current.page);
    let pages = current.max_pages;
    if request.filter.after_index.is_some() || !(1..=1000).contains(&pages) { return Err(limit()); }
    let mut checkpoint = None;
    let result = run(ctx, current, request, pages, &mut inspect, &mut checkpoint);
    match result {
        Ok(mut out) => {
            out.warnings.push(json!(WARNING));
            Ok(out)
        }
        Err(error) => Err(checkpoint_error(error, checkpoint)),
    }
}
fn run(
    ctx: &DataContext,
    current: &ScanRequest,
    query: Request,
    max_pages: usize,
    inspect: &mut impl FnMut(&DataContext, &LocalRequest) -> Result<Outcome>,
    checkpoint: &mut Option<Outcome>,
) -> Result<Outcome> {
    let mut state = State {
        snapshot: current.page.snapshot.clone(),
        pages: Vec::new(),
        ..Default::default()
    };
    let resume = &current.cursor;
    if !resume.is_empty() {
        if !query.file.is_empty() {
            return Err(identity("Resume requires an archived --from"));
        }
        let budget = ledger(ctx);
        let reference =
            evidence::read_capture_controlled_with_check(&ctx.home, resume, &ctx.control, || {
                budget.check()
            })?;
        budget.check()?;
        let raw = evidence::read_blob_bounded_with_check(
            &ctx.home,
            &reference["blob"],
            MANIFEST_BYTES,
            &ctx.control,
            || budget.check(),
        )?;
        budget.charge(Cost::metadata(raw.len()))?;
        let _parser = budget.reserve_retained(raw.len().saturating_mul(64).saturating_add(4096))?;
        let saved: State =
            serde_json::from_slice(&raw).map_err(|_| identity("Invalid scan checkpoint"))?;
        let saved_source = saved
            .source
            .as_ref()
            .ok_or_else(|| identity("Scan checkpoint has no source"))?;
        if reference["provenance"]["kind"] != "hotfix-scan"
            || saved.snapshot != state.snapshot
            || saved_source.id.as_str() != query.from
            || !same_request(&saved.query, &query)
        {
            return Err(identity("Resume source, snapshot or filters changed"));
        }
        // The parse reservation already admits the bounded typed state and its
        // public tree. Install the checkpoint before the next cancellation
        // check, matching Go's deferred saved-reading error path.
        let public = serde_json::to_value(&saved)?;
        *checkpoint = Some(outcome(public, reference, true));
        // Even complete historical checkpoints can have lost dependencies.
        // Preserve their exact resume identity while reporting that failure.
        for id in
            std::iter::once(saved_source.id.as_str()).chain(saved.pages.iter().map(String::as_str))
        {
            budget.check()?;
            evidence::read_capture_controlled_with_check(&ctx.home, id, &ctx.control, || {
                budget.check()
            })?;
        }
        state = saved;
        if state.complete {
            let mut out = checkpoint.take().expect("validated saved checkpoint");
            out.data
                .as_object_mut()
                .expect("scan reading")
                .remove("resume");
            return Ok(out);
        }
    } else {
        state.query = query.clone();
    }
    for _ in 0..max_pages {
        ctx.control.check()?;
        admit_page(&state)?;
        let mut page = inspect(ctx, &page_request(&state, query.offline))?;
        let reference = page
            .provenance
            .pop()
            .ok_or_else(|| identity("Hotfix page has no capture"))?;
        let page: PageResult = serde_json::from_value(page.data)?;
        if state.pages.len() > 0
            && (page.source.id.as_str()
                != state.source.as_ref().expect("scan has source").id.as_str()
                || page.page.matched != state.matched)
        {
            return Err(identity("Immutable source or matched coverage changed"));
        }
        state.snapshot = page.snapshot;
        state.source = Some(page.source);
        state.matched = page.page.matched;
        state.query.file.clear();
        state.query.from = state
            .source
            .as_ref()
            .expect("scan has source")
            .id
            .as_str()
            .into();
        for entry in page.page.entries {
            if state.next_index.is_some_and(|index| entry.index <= index) {
                return Err(cursor());
            }
            state.next_index = Some(entry.index);
            state.returned = state.returned.wrapping_add(1);
            let counter = match entry.decode_state.as_str() {
                "decoded" => &mut state.decoded,
                "no_payload" => &mut state.no_payload,
                "not_valid" => &mut state.not_valid,
                _ => &mut state.raw,
            };
            *counter = counter.wrapping_add(1);
        }
        state.pages.push(
            reference["id"]
                .as_str()
                .ok_or_else(|| identity("Hotfix page capture has no identity"))?
                .into(),
        );
        state.truncated = page.page.truncated;
        state.complete = !state.truncated && state.returned == state.matched;
        if !state.truncated && !state.complete
            || state.truncated
                && (page.page.next_index.is_none()
                    || state.next_index.is_none()
                    || page.page.next_index != state.next_index)
        {
            return Err(cursor());
        }
        if state.complete {
            state.next_index = None;
        }
        let budget = ledger(ctx);
        let (encoded, public) = encoded_state(&state, &budget)?;
        budget.charge(Cost::retained(
            (state.pages.len() + 1).saturating_mul(size_of::<&str>()),
        ))?;
        let dependencies: Vec<&str> =
            std::iter::once(state.source.as_ref().expect("scan has source").id.as_str())
                .chain(state.pages.iter().map(String::as_str))
                .collect();
        let accounting = ScanCaptureAccounting::new(&budget);
        let capture = evidence::commit_capture_budgeted_with_dependencies(
            &ctx.home,
            evidence::CaptureInput {
                bytes: &encoded,
                maximum_bytes: MANIFEST_BYTES,
                media_type: "application/json",
                provenance: json!({"kind":"hotfix-scan", "locator":state.source.as_ref().expect("scan has source").id.as_str(), "snapshot":state.snapshot, "dataBuild":state.source.as_ref().expect("scan has source").provenance.data_build.as_str()}),
                complete: state.complete,
                truncated: state.truncated,
            },
            &dependencies,
            &ctx.control,
            &accounting,
        )?;
        *checkpoint = Some(outcome(public, capture, true));
        if state.complete {
            break;
        }
    }
    let mut out = checkpoint.take().expect("at least one page published");
    if state.complete {
        out.data
            .as_object_mut()
            .expect("scan reading")
            .remove("resume");
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
