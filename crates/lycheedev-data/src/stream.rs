//! Typed DB2 streaming application and its fixed public wire adapter.
//!
//! The row executor receives only an admitted table, projection, filter, limit
//! and budget. The application still lends the original Invocation to the
//! existing provider: its pointer identity and shared budget are admission
//! credentials, not values that may be reconstructed from the typed request.
mod application;
mod execute;
pub mod wire;

use crate::{
    definitions::TableIdentity,
    fault,
    resource::{Cost, MemoryBudget},
    schema, table,
};
use lycheedev_core::{Invocation, Outcome, Result, selection::DataPin};
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};

/// Parsed once at the command boundary. Field resolution deliberately remains
/// after provider preparation and table opening, preserving first-error order.
pub struct StreamRequest<'a> {
    invocation: &'a Invocation,
    name: &'a str,
    fields: Option<&'a str>,
    filter_text: &'a str,
    filter: Option<Filter<'a>>,
    limit: Limit,
    timeout: Duration,
    cdn: bool,
}
impl<'a> StreamRequest<'a> {
    pub fn from_invocation(inv: &'a Invocation) -> Result<Self> {
        inv.control.check()?;
        if inv.flag("--format") != Some("jsonl") {
            return Err(fault(
                "command.invalid_arguments",
                2,
                "data db2 stream requires --format jsonl",
            ));
        }
        let name = inv.argument()?;
        inv.required("--limit")?;
        let limit = Limit(inv.usize_flag("--limit", 0, 1, 200000)?);
        let filter_text = inv.flag("--filter").unwrap_or("");
        let filter = Filter::parse(filter_text)?;
        let timeout = inv.usize_flag("--timeout-seconds", 300, 1, 3600)?;
        Ok(Self {
            invocation: inv,
            name,
            fields: inv.flag("--fields").filter(|text| !text.is_empty()),
            filter_text,
            filter,
            limit,
            timeout: Duration::from_secs(timeout as u64),
            cdn: inv.flag_bool("--cdn"),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit(usize);
impl Limit {
    pub fn get(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Filter<'a> {
    field: &'a str,
    value: &'a str,
}
impl<'a> Filter<'a> {
    fn parse(text: &'a str) -> Result<Option<Self>> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        let invalid = || {
            fault(
                "records.field_type_mismatch",
                2,
                format!(
                    "records.field_type_mismatch: invalid filter {text:?}, expected field=value"
                ),
            )
            .with_details(wire::query_stage())
        };
        let (field, value) = text.split_once('=').ok_or_else(invalid)?;
        let field = field.trim();
        if field.is_empty() {
            return Err(invalid());
        }
        Ok(Some(Self {
            field,
            value: value.trim(),
        }))
    }
}

#[derive(Debug, Clone)]
pub struct Projection {
    fields: Vec<String>,
}
impl Projection {
    fn resolve(
        text: Option<&str>,
        filter: Option<Filter<'_>>,
        definition: &schema::Definition,
    ) -> Result<Self> {
        let fields: Vec<String> = match text {
            Some(text) => text
                .split(',')
                .map(|value| value.trim().to_owned())
                .collect(),
            None => definition
                .fields
                .iter()
                .map(|field| field.name.clone())
                .collect(),
        };
        if fields.len() > 1024 {
            return Err(fault(
                "records.query_resource_budget",
                3,
                "Stream projection field budget exceeded",
            ));
        }
        for name in fields
            .iter()
            .map(String::as_str)
            .chain(filter.iter().map(|filter| filter.field))
        {
            if !definition.fields.iter().any(|field| field.name == name) {
                return Err(field_error(name));
            }
        }
        Ok(Self { fields })
    }
    pub fn fields(&self) -> &[String] {
        &self.fields
    }
}

/// The two dynamic values are exact metadata from the existing file provider.
/// They are not consulted for application control or row execution.
#[derive(Debug, Clone)]
pub struct TableProof {
    pub key_source: Value,
    pub requested_name: String,
    pub identity: TableIdentity,
    pub layout_hash: String,
    pub definition_sha256: String,
    pub content_key: Value,
    pub logical_rows: usize,
}
#[derive(Debug, Clone)]
pub struct StreamBegin {
    pub proof: TableProof,
    pub snapshot: String,
    pub source: Value,
    pub projection: Projection,
    pub filter: String,
    pub limit: Limit,
}

/// Borrowed only for the synchronous sink call. No frame owns another row, and
/// the executor cannot decode the next row until this callback returns.
#[derive(Debug, Clone, Copy)]
pub struct StreamRecord<'a> {
    pub index: usize,
    pub record_id: u32,
    pub row: &'a BTreeMap<String, table::Value>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkSummary {
    pub count: usize,
    pub truncated: bool,
}
impl WalkSummary {
    pub fn complete(self) -> bool {
        !self.truncated
    }
    pub fn truncation_reason(self) -> &'static str {
        if self.truncated {
            "limit_reached"
        } else {
            "none"
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub enum StreamEvent<'a> {
    Begin(&'a StreamBegin),
    Record(StreamRecord<'a>),
    End(WalkSummary),
    Error { code: &'a str, message: &'a str },
}

/// Returned only after checked close and a successful terminal sink call.
/// Snapshot accounting is captured before manifest publication, as in Go323.
#[derive(Debug)]
pub struct StreamSummary {
    pub begin: StreamBegin,
    pub pin: DataPin,
    pub resources: Cost,
    pub memory_budget: MemoryBudget,
    pub resource_usage: Cost,
    pub walk: WalkSummary,
    pub captures: Vec<Value>,
}

pub fn run(
    request: StreamRequest<'_>,
    sink: &mut dyn FnMut(StreamEvent<'_>) -> Result<()>,
) -> Result<StreamSummary> {
    application::run_after_finish(request, sink, &mut |result| result)
}

/// Compatibility entry point for callers of the original JSON frame API.
/// Both this adapter and the CLI consume the same typed application and renderer.
pub fn stream(inv: &Invocation, sink: &mut dyn FnMut(&Value) -> Result<()>) -> Result<Outcome> {
    run(StreamRequest::from_invocation(inv)?, &mut |event| {
        sink(&wire::event(event))
    })
    .map(wire::into_outcome)
}

/// Narrow post-close observation seam; the real consuming close still runs.
#[cfg(test)]
pub(crate) fn stream_after_finish(
    inv: &Invocation,
    sink: &mut dyn FnMut(&Value) -> Result<()>,
    after_finish: &mut impl FnMut(Result<StreamSummary>) -> Result<StreamSummary>,
) -> Result<Outcome> {
    application::run_after_finish(
        StreamRequest::from_invocation(inv)?,
        &mut |event| sink(&wire::event(event)),
        after_finish,
    )
    .map(wire::into_outcome)
}

fn field_error(name: &str) -> lycheedev_core::ToolkitError {
    fault(
        "records.field_unknown",
        2,
        format!("records.field_unknown: {name}"),
    )
    .with_details(wire::query_stage())
}

#[cfg(test)]
mod tests;
