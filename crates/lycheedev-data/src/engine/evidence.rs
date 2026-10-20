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
    control.check()?;
    let budget = &lifetime.budget;
    // One row's conversion scratch is reused, separate from the bounded body.
    let scratch = budget.reserve_retained(MAXIMUM)?;
    // Core capture documents have a fixed 1 MiB bound; retain the parsed receipt.
    let receipt = budget.reserve_retained(32 << 20)?.into_owned(budget)?;
    let error = RefCell::new(None);
    let body = Body {
        result,
        budget,
        error: &error,
    };
    let encoded = capture::json_bytes_owned(&body, budget, MAXIMUM, limit);
    let (raw, allocation) = match encoded {
        Ok(value) => value,
        Err(failure) => return Err(error.into_inner().unwrap_or(failure)),
    };
    drop(scratch);
    let request_digest = capture::json_sha256(
        &result.provenance["request"],
        budget,
        serde_json::ser::CompactFormatter,
    )?;
    let references = result
        .dependencies
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let committed = evidence::commit_capture_budgeted_with_dependencies(
        &invocation.home,
        evidence::CaptureInput {
            bytes: &raw,
            maximum_bytes: MAXIMUM,
            media_type: "application/json",
            provenance: serde_json::json!({"kind":"data-agent-query", "locator":format!("typed:sha256:{request_digest}"),
                "snapshot":invocation.flags.get("--snapshot")}),
            complete: result.complete,
            truncated: false,
        },
        &references,
        &control,
        budget.as_ref(),
    );
    // Never release the body admission before its backing bytes.
    drop(raw);
    drop(allocation);
    let value = committed?;
    if let Err(mut error) = lifetime.settle() {
        let primary = error.clone();
        error.details = serde_json::json!({"schema":"lycheedev.capture-commit-error.v1", "primary":primary,
            "capturePublication":{"state":"committed","id":value["id"]}, "completedCaptures":[value["id"]]});
        return Err(error);
    }
    Ok(OwnedCapture {
        value,
        allocation: Some(receipt),
        lifetime,
    })
}
