//! Retirement-only validation, not source authentication or query reuse.
//! All known manifests are projected before Core obtains deletion authority.
//! The existing Receipt supplies the exact outer schema; no second wire format.
use super::{
    application,
    reading::{Receipt, invalid},
};
use crate::resource::{Budget, Cost};
use lycheedev_core::{
    Invocation, Outcome, Result, ToolkitError,
    evidence::BlobRef,
    selection, workspace,
    workspace::{
        data_disk::{DerivedObject, DerivedRetirementOptions},
        data_resources::{self, Policy},
    },
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::value::RawValue;

fn strict<T: DeserializeOwned + Serialize>(raw: &RawValue, budget: &Budget) -> Result<T> {
    let value: T = serde_json::from_str(raw.get()).map_err(|_| invalid())?;
    // Compare against the existing producer's canonical representation, without
    // allocating two JSON DOMs. Unknown nested fields cannot disappear silently.
    super::reading::canonical_matches(&value, raw.get().as_bytes(), budget)?;
    Ok(value)
}
fn digest(value: &str, length: usize) -> bool {
    selection::valid_digest(value, length) && !value.bytes().any(|b| b.is_ascii_uppercase())
}
fn blob(raw: &RawValue, required: bool) -> Result<Option<BlobRef>> {
    let value: Option<BlobRef> = serde_json::from_str(raw.get()).map_err(|_| invalid())?;
    match value {
        // Historical source BlobRefs accept uppercase digests. This projection
        // does not use their text as a managed deletion path or object key.
        Some(value)
            if value.sha256.len() == 64
                && value.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
                && value.bytes > 0 =>
        {
            Ok(Some(value))
        }
        Some(value) if !required && value.sha256.is_empty() && value.bytes == 0 => Ok(None),
        None if !required => Ok(None),
        _ => Err(invalid()),
    }
}
pub(super) fn reference(key: &str, raw: &[u8], budget: &Budget) -> Result<DerivedObject> {
    budget.check()?;
    let _scratch = budget.reserve_retained(
        raw.len()
            .checked_mul(8)
            .and_then(|n| n.checked_add(128 << 10))
            .ok_or_else(invalid)?,
    )?;
    budget.charge(Cost::work(raw.len()))?;
    let receipt: Receipt<'_> = serde_json::from_slice(raw).map_err(|_| invalid())?;
    if receipt.schema != "lycheedev.data-static-generation.v1"
        || receipt.identity_sha256 != key || !digest(key, 64)
        || receipt.source_kind != "static"
        || receipt.representation != application::REPRESENTATION
        || receipt.derivation_version != application::DERIVATION_VERSION
        || !digest(receipt.definition_sha256, 64) || !digest(receipt.typed_schema_sha256, 64)
        || !digest(receipt.object.sha256, 64) || receipt.object.bytes == 0
        // Same explicit prepare maximum: --max-bytes <=512MiB, IPC <=4x+16MiB.
        || receipt.object.bytes > (512u64 << 20) * 4 + (16 << 20)
        || receipt.record_id_order != "ascending" || (receipt.record_id_column == 0 || receipt.record_id_column > 4096)
        || receipt.batch_count > receipt.available_rows
        || (receipt.available_rows != 0 && receipt.batch_count == 0)
    {
        return Err(invalid());
    }
    let data: selection::DataPin = strict(receipt.data, budget)?;
    if data.product.is_empty()
        || data.region.is_empty()
        || data.language.is_empty()
        || !selection::valid_build(&data.full_build)
        || !selection::valid_digest(&data.build_config, 32)
        || !selection::valid_digest(&data.cdn_config, 32)
        || !selection::valid_digest(&data.definition_commit, 40)
    {
        return Err(invalid());
    }
    let table: crate::definitions::TableIdentity = strict(receipt.table, budget)?;
    if !crate::definitions::valid_name(&table.name) {
        return Err(invalid());
    }
    let missing: Vec<crate::blte::Missing> = strict(receipt.missing, budget)?;
    let partitions: Vec<usize> = strict(receipt.unavailable_partitions, budget)?;
    if partitions.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(invalid());
    }
    let source = blob(receipt.source_content, receipt.complete)?;
    let partial = blob(receipt.partial_content, !receipt.complete)?;
    let selected = if receipt.complete { source } else { partial }.ok_or_else(invalid)?;
    if receipt.complete && (!missing.is_empty() || !partitions.is_empty()) {
        return Err(invalid());
    }
    let mut previous_end = 0;
    for item in &missing {
        budget.check()?;
        let end = item
            .offset
            .checked_add(item.bytes)
            .filter(|end| *end as u64 <= selected.bytes)
            .ok_or_else(invalid)?;
        if item.bytes == 0 || item.offset < previous_end || !digest(&item.key_id, 16) {
            return Err(invalid());
        }
        previous_end = end;
    }
    if !blob(receipt.definition_object, true)?
        .ok_or_else(invalid)?
        .sha256
        .eq_ignore_ascii_case(receipt.definition_sha256)
    {
        return Err(invalid());
    }
    blob(receipt.definition_manifest, true)?;
    budget.check()?;
    Ok(DerivedObject {
        sha256: receipt.object.sha256.into(),
        bytes: receipt.object.bytes,
    })
}

pub(crate) fn prune(inv: &Invocation) -> Result<Outcome> {
    if inv.command != "cache prune" || !inv.flag_bool("--data-derived") {
        return Err(ToolkitError::arguments(
            "Data retirement requires cache prune --data-derived",
        ));
    }
    if inv.flag_bool("--uncommitted") {
        return Err(ToolkitError::arguments(
            "--data-derived does not include --uncommitted staging",
        ));
    }
    let options = DerivedRetirementOptions {
        target_bytes: inv.usize_flag("--target-bytes", 0, 0, usize::MAX)? as u64,
        max_objects: inv.usize_flag("--max-objects", 0, 0, 100000)?,
        dry_run: inv.flag_bool("--dry-run"),
    };
    workspace::open_controlled(&inv.home, &inv.control)?;
    let budget = Budget::query_controlled(inv.control.deadline(), inv.control.clone());
    let resources =
        data_resources::acquire_with_check(&inv.home, Policy::Ordinary, &inv.control, || {
            budget.check()
        })?;
    let result = (|| {
        budget.initialize_admitted(&resources)?;
        resources.retire_derived_cache_with_check(
            options,
            &|| budget.check(),
            |bytes| budget.reserve_retained(bytes),
            |key, bytes| reference(key, bytes, &budget),
        )
    })();
    // Close the original query owner before any fallible result serialization.
    // Preserve completed deletion facts even when that final checked close fails.
    let cleanup = resources.close_with_check(|| budget.check());
    match (result, cleanup) {
        (Ok(report), Ok(())) => {
            let complete = report.complete;
            let mut outcome = Outcome::new(serde_json::to_value(report)?);
            outcome.complete = complete;
            Ok(outcome)
        }
        (Ok(mut report), Err(mut error)) => {
            report.complete = false;
            error.details = serde_json::json!({"schema":"lycheedev.data-derived-prune-error.v1", "primary":error.clone(), "result":report});
            Err(error)
        }
        (Err(error), cleanup) => {
            lycheedev_core::metadata::historical_write::finish_historical(Err(error), cleanup)
        }
    }
}
#[cfg(test)]
mod tests;
