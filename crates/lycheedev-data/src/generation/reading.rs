//! Reuse only a complete, authenticated publication for the freshly prepared key.
//! The original source owner remains alive; no SQL/effective/Hotfix route changes.
mod ipc;
#[cfg(test)]
mod tests;

use super::{StaticIdentity, application};
use crate::{
    db2::Prepared,
    resource::{Budget, Cost},
    table::Table,
};
use arrow_schema::SchemaRef;
use lycheedev_core::{
    Result, ToolkitError,
    workspace::data_disk::{DerivedObject, DerivedReadLease},
};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::io::{self, Write};

pub(super) fn invalid() -> ToolkitError {
    crate::fault(
        "records.generation_invalid",
        4,
        "Stored typed generation differs from its authenticated publication contract",
    )
}
fn limit() -> ToolkitError {
    crate::fault(
        "records.query_resource_budget",
        3,
        "Stored typed generation exceeds its admitted read budget",
    )
}
fn plus(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or_else(limit)
}
fn times(a: usize, b: usize) -> Result<usize> {
    a.checked_mul(b).ok_or_else(limit)
}

// Match the existing source-cache convention: one unit per bounded 64KiB
// bulk I/O/authentication block, admitted for the entire pass before work.
// This does not replace byte bounds, allocation reservations, per-block
// cancellation or semantic manifest/value/ID validation work.
fn admit_bulk_work(budget: &Budget, bytes: usize) -> Result<()> {
    budget.charge(Cost::work(bytes.div_ceil(65536)))
}

// Borrow metadata from the bounded input, without building an unbounded JSON DOM.
// Exact canonical comparison below binds EVERY field, including raw subobjects.
#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt<'a> {
    schema: &'a str,
    identity_sha256: &'a str,
    source_kind: &'a str,
    #[serde(borrow)]
    data: &'a RawValue,
    #[serde(borrow)]
    table: &'a RawValue,
    definition_sha256: &'a str,
    typed_schema_sha256: &'a str,
    #[serde(borrow)]
    object: Object<'a>,
    representation: &'a str,
    derivation_version: &'a str,
    available_rows: usize,
    batch_count: usize,
    complete: bool,
    #[serde(borrow)]
    missing: &'a RawValue,
    #[serde(borrow)]
    unavailable_partitions: &'a RawValue,
    record_id_column: usize,
    record_id_order: &'a str,
    #[serde(borrow)]
    source_content: &'a RawValue,
    #[serde(borrow)]
    partial_content: &'a RawValue,
    #[serde(borrow)]
    definition_object: &'a RawValue,
    #[serde(borrow)]
    definition_manifest: &'a RawValue,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Object<'a> {
    sha256: &'a str,
    bytes: u64,
}

struct Compare<'a> {
    input: &'a [u8],
    offset: usize,
    budget: &'a Budget,
    error: Option<ToolkitError>,
}
impl Write for Compare<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let result = (|| {
            self.budget.charge(Cost::work(bytes.len()))?;
            let end = plus(self.offset, bytes.len())?;
            if self.input.get(self.offset..end) != Some(bytes) {
                return Err(invalid());
            }
            self.offset = end;
            Ok(bytes.len())
        })();
        result.map_err(|error| {
            self.error = Some(error);
            io::Error::other("Generation manifest comparison stopped")
        })
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn canonical_matches(expected: &impl Serialize, bytes: &[u8], budget: &Budget) -> Result<()> {
    let mut compare = Compare {
        input: bytes,
        offset: 0,
        budget,
        error: None,
    };
    if serde_json::to_writer(&mut compare, expected).is_err() {
        return Err(compare.error.unwrap_or_else(invalid));
    }
    if compare.offset != bytes.len() {
        return Err(invalid());
    }
    budget.check()
}

pub(super) struct Hit {
    pub(super) object: DerivedObject,
    pub(super) rows: usize,
    pub(super) batches: usize,
}
#[allow(clippy::too_many_arguments)]
pub(super) fn read(
    owner: &DerivedReadLease<'_>,
    prepared: &Prepared,
    identity: &StaticIdentity<'_>,
    key: &str,
    schema_sha256: &str,
    schema: &SchemaRef,
    table: &Table<'_>,
    maximum_object_bytes: u64,
    budget: &Budget,
) -> Result<Option<Hit>> {
    budget.check()?;
    // Includes fixed raw bytes, serde's bounded borrowed-field parsing scratch,
    // possible escaped-string scratch growth, and hashing/I/O scratch. Core
    // never allocates more than the 1MiB raw-input bound.
    let _receipt_allocation = budget.reserve_retained((4 << 20) + (128 << 10))?;
    let Some(bytes) = owner.read_generation(key)? else {
        return Ok(None);
    };
    budget.charge(Cost::work(bytes.len()))?;
    let receipt: Receipt<'_> = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if receipt.available_rows != table.logical_count()
        || receipt.batch_count > receipt.available_rows
        || (receipt.available_rows != 0 && receipt.batch_count == 0)
        || receipt.object.bytes > maximum_object_bytes
        || receipt.object.bytes == 0
        || !lycheedev_core::selection::valid_digest(receipt.object.sha256, 64)
        || receipt
            .object
            .sha256
            .bytes()
            .any(|b| b.is_ascii_uppercase())
    {
        return Err(invalid());
    }
    let object = DerivedObject {
        sha256: receipt.object.sha256.to_owned(),
        bytes: receipt.object.bytes,
    };
    let expected = application::manifest(
        prepared,
        identity,
        key,
        schema_sha256,
        &object,
        receipt.available_rows,
        receipt.batch_count,
        schema.fields().len() - 1,
    );
    canonical_matches(&expected, &bytes, budget)?;
    // Admit the complete authentication pass before Core hashes this object.
    admit_bulk_work(budget, usize::try_from(object.bytes).map_err(|_| limit())?)?;
    let mut reader = owner.open_object(&object.sha256, object.bytes)?;
    let result = ipc::validate(&mut reader, schema, table, receipt.batch_count, budget);
    let authenticated = result.is_ok();
    reader.close_after(result).map_err(|mut error| {
        if authenticated {
            error.details = serde_json::json!({
                "primary":error.details,
                "existingGeneration":{
                    "generation":key,"object":object,"authentication":"complete",
                    "sourceVerification":"pending","publicationThisCommand":false,
                },
            });
        }
        error
    })?;
    budget.check()?;
    Ok(Some(Hit {
        object,
        rows: receipt.available_rows,
        batches: receipt.batch_count,
    }))
}
