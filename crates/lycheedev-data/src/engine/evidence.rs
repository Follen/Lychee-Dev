//! Explicit complete-result evidence task. Query handles alone are not CAPs.
use super::{lifetime::RequestLifetime, result::ResultOwner};
use crate::{
    capture, fault,
    resource::{Budget, OwnedRetainedReservation},
};
use lycheedev_core::{Invocation, Result, ToolkitError, control::OperationControl, evidence};
use serde::{
    Serialize, Serializer,
    ser::{Error as _, SerializeSeq, SerializeStruct},
};
use serde_json::Value;
use std::{cell::RefCell, sync::Arc};
const MAXIMUM: usize = 16 << 20;

#[derive(Serialize)]
#[serde(transparent)]
pub struct OwnedCapture {
    value: Value,
    #[serde(skip)]
    allocation: Option<OwnedRetainedReservation>,
    #[serde(skip)]
    lifetime: Arc<RequestLifetime>,
}
impl OwnedCapture {
    /// Transport retains committed identity if serialization/flush fails later.
    pub(crate) fn committed_error(&self, error: ToolkitError) -> ToolkitError {
        committed_error(error, &self.value)
    }
}
impl std::fmt::Debug for OwnedCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnedCapture")
            .field("id", &self.value.get("id"))
            .finish_non_exhaustive()
    }
}
impl Drop for OwnedCapture {
    fn drop(&mut self) {
        drop(std::mem::replace(&mut self.value, Value::Null));
        self.allocation.take();
        let _ = self.lifetime.settle();
    }
}
fn limit() -> ToolkitError {
    fault(
        "records.query_resource_budget",
        3,
        "Complete result exceeds capture admission; no truncated CAP was emitted",
    )
}
struct Rows<'a> {
    result: &'a ResultOwner,
    budget: &'a Budget,
    error: &'a RefCell<Option<ToolkitError>>,
}
impl Serialize for Rows<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.result.rows))?;
        for batch in &self.result.batches {
            for row in 0..batch.num_rows() {
                let values = (|| {
                    self.budget.check()?;
                    self.budget.charge(crate::resource::Cost::work(
                        batch.num_columns().saturating_add(1),
                    ))?;
                    let mut remaining = MAXIMUM;
                    let bytes = batch.num_columns().checked_mul(64).ok_or_else(limit)?;
                    remaining = remaining.checked_sub(bytes).ok_or_else(limit)?;
                    let mut values = Vec::with_capacity(batch.num_columns());
                    for column in batch.columns() {
                        self.budget.check()?;
                        values.push(super::result::value(
                            column.as_ref(),
                            row,
                            &mut remaining,
                            0,
                        )?);
                    }
                    Ok::<_, ToolkitError>(values)
                })()
                .map_err(|error| {
                    *self.error.borrow_mut() = Some(error);
                    S::Error::custom("Complete result serialization stopped")
                })?;
                sequence.serialize_element(&values)?;
            }
        }
        sequence.end()
    }
}
struct Body<'a> {
    result: &'a ResultOwner,
    budget: &'a Budget,
    error: &'a RefCell<Option<ToolkitError>>,
}
impl Serialize for Body<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut object = serializer.serialize_struct("DataAgentResult", 6)?;
        object.serialize_field("schema", "lycheedev.data-agent-result.v1")?;
        object.serialize_field("columns", &self.result.columns)?;
        object.serialize_field(
            "rows",
            &Rows {
                result: self.result,
                budget: self.budget,
                error: self.error,
            },
        )?;
        object.serialize_field("rowCount", &self.result.rows)?;
        object.serialize_field("complete", &self.result.complete)?;
        object.serialize_field("provenance", &self.result.provenance)?;
        object.end()
    }
}

pub(super) fn capture(
    invocation: &Invocation,
    result: &ResultOwner,
    lifetime: Arc<RequestLifetime>,
    control: OperationControl,
) -> Result<OwnedCapture> {
    let error = RefCell::new(None);
    let body = Body {
        result,
        budget: &lifetime.budget,
        error: &error,
    };
    let references = result
        .dependencies
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    commit_body(
        invocation,
        &body,
        &result.provenance["request"],
        &references,
        result.complete,
        false,
        "data-agent-query",
        MAXIMUM,
        &error,
        lifetime.clone(),
        control,
    )
}
#[derive(Serialize)]
struct DomainBody<'a> {
    schema: &'static str,
    request: &'a super::api::DomainRequest,
    result: &'a super::domain::DomainResult,
    complete: bool,
}
pub(super) fn capture_domain(
    invocation: &Invocation,
    result: &super::domain::DomainResult,
    request: &super::api::DomainRequest,
    lifetime: Arc<RequestLifetime>,
    control: OperationControl,
) -> Result<OwnedCapture> {
    control.check()?;
    let complete = result.complete()?;
    let truncated = result.truncated()?;
    let count = result.sources().iter().try_fold(0usize, |n, source| {
        n.checked_add(source.dependencies().len()).ok_or_else(limit)
    })?;
    let reference_allocation = lifetime.budget.reserve_retained(
        count
            .checked_mul(std::mem::size_of::<&str>())
            .ok_or_else(limit)?,
    )?;
    let mut references = result
        .sources()
        .iter()
        .flat_map(|source| source.dependencies().iter().map(String::as_str))
        .collect::<Vec<_>>();
    references.sort_unstable();
    references.dedup();
    let body = DomainBody {
        schema: "lycheedev.data-agent-domain-result.v1",
        request,
        result,
        complete,
    };
    let error = RefCell::new(None);
    let outcome = commit_body(
        invocation,
        &body,
        request,
        &references,
        complete,
        truncated,
        "data-agent-domain",
        0,
        &error,
        lifetime.clone(),
        control,
    );
    drop(references);
    drop(reference_allocation);
    // The common commit may have settled while this short-lived index existed.
    // Reconcile after its real release; retain committed identity on failure.
    if let Ok(ref receipt) = outcome {
        if let Err(error) = lifetime.settle() {
            return Err(committed_error(error, &receipt.value));
        }
    }
    outcome
}
fn committed_error(mut error: ToolkitError, value: &Value) -> ToolkitError {
    let primary = error.clone();
    error.details = serde_json::json!({"schema":"lycheedev.capture-commit-error.v1", "primary":primary,
        "capturePublication":{"state":"committed","id":value["id"]}, "completedCaptures":[value["id"]]});
    error
}
fn commit_body(
    invocation: &Invocation,
    body: &impl Serialize,
    request: &impl Serialize,
    references: &[&str],
    complete: bool,
    truncated: bool,
    kind: &'static str,
    scratch_bytes: usize,
    error: &RefCell<Option<ToolkitError>>,
    lifetime: Arc<RequestLifetime>,
    control: OperationControl,
) -> Result<OwnedCapture> {
    control.check()?;
    let budget = &lifetime.budget;
    let scratch = budget.reserve_retained(scratch_bytes)?;
    let receipt = budget.reserve_retained(32 << 20)?.into_owned(budget)?;
    let (raw, allocation) = capture::json_bytes_owned(body, budget, MAXIMUM, limit)
        .map_err(|failure| error.borrow_mut().take().unwrap_or(failure))?;
    drop(scratch);
    let request_digest = capture::json_sha256(request, budget, serde_json::ser::CompactFormatter)?;
    let committed = evidence::commit_capture_budgeted_with_dependencies(
        &invocation.home,
        evidence::CaptureInput {
            bytes: &raw,
            maximum_bytes: MAXIMUM,
            media_type: "application/json",
            provenance: serde_json::json!({"kind":kind, "locator":format!("typed:sha256:{request_digest}"),
                "snapshot":invocation.flags.get("--snapshot")}),
            complete,
            truncated,
        },
        references,
        &control,
        budget.as_ref(),
    );
    drop(raw);
    drop(allocation);
    let value = committed?;
    lifetime
        .settle()
        .map_err(|error| committed_error(error, &value))?;
    Ok(OwnedCapture {
        value,
        allocation: Some(receipt),
        lifetime,
    })
}
