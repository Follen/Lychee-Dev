//! Fixed Go records.InspectHotfix's local named-table path. Scan checkpoints and
//! remote providers stay separate and explicitly unavailable in this batch.
use super::{CacheFilter, decode_page, inspect_budgeted};
use crate::{
    capture, definitions, fault,
    resource::{Budget, Cost, RetainedReservation},
    schema,
};
use lycheedev_core::{Invocation, Outcome, Result, ToolkitError, evidence, selection, storage};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

fn identity(message: &str) -> ToolkitError {
    fault("records.hotfix_identity", 4, message)
}
fn limit() -> ToolkitError {
    fault(
        "records.hotfix_limit",
        3,
        "Hotfix input or output exceeds its bound",
    )
}
struct Files<'a> {
    budget: &'a Budget,
    temporary: Vec<RetainedReservation<'a>>,
}
impl storage::FileControl for Files<'_> {
    type Error = ToolkitError;
    fn charge(&mut self, cost: storage::FileCost) -> Result<()> {
        self.budget.charge(Cost {
            decode_work: cost.work_units.saturating_add(cost.io_operations),
            ..Default::default()
        })?;
        if cost.allocation_bytes > 0 {
            let bytes = usize::try_from(cost.allocation_bytes)
                .ok()
                .and_then(|bytes| bytes.checked_add(4 * size_of::<RetainedReservation<'_>>()))
                .ok_or_else(limit)?;
            let reservation = self.budget.reserve_retained(bytes)?;
            self.temporary.try_reserve(1).map_err(|_| limit())?;
            self.temporary.push(reservation);
        }
        Ok(())
    }
    fn check(&mut self) -> Result<()> {
        self.budget.check()
    }
}
fn read_file(path: &Path, maximum: usize, budget: &Budget) -> Result<Vec<u8>> {
    budget.check()?;
    let mut files = Files {
        budget,
        temporary: Vec::new(),
    };
    let parent_path = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = storage::Directory::open_controlled(parent_path, &mut files)?;
    let name = path
        .file_name()
        .ok_or_else(|| identity("Cache path requires a file name"))?;
    use std::os::windows::fs::MetadataExt;
    let observed = parent.metadata_controlled(name, &mut files)?;
    if !observed.is_file() || observed.file_attributes() & 0x400 != 0 {
        return Err(fault(
            "records.hotfix_format",
            4,
            "Cache must be a regular non-reparse file",
        ));
    }
    if observed.len() > maximum as u64 {
        return Err(limit());
    }
    let mut file = parent.open_file_controlled(name, &mut files)?;
    budget.charge(Cost::work(1))?;
    let before = file.metadata()?;
    budget.check()?;
    if !before.is_file() || before.file_attributes() & 0x400 != 0 {
        return Err(fault(
            "records.hotfix_format",
            4,
            "Cache must be a regular non-reparse file",
        ));
    }
    let size = usize::try_from(before.len()).map_err(|_| limit())?;
    if size > maximum {
        return Err(limit());
    }
    budget.charge(Cost::retained(size))?;
    let mut raw = Vec::new();
    raw.try_reserve_exact(size).map_err(|_| limit())?;
    let _scratch = budget.reserve_retained(65536)?;
    let mut block = [0u8; 65536];
    loop {
        budget.check()?;
        let count = match file.read(&mut block) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        budget.charge(Cost {
            metadata_bytes: count as u64,
            decode_work: count.div_ceil(65536) as u64 + 1,
            ..Default::default()
        })?;
        if count == 0 {
            break;
        }
        if count > maximum - raw.len() {
            return Err(limit());
        }
        if count > size - raw.len() {
            return Err(identity("Cache changed while reading; retry a stable copy"));
        }
        raw.extend_from_slice(&block[..count]);
    }
    budget.charge(Cost::work(1))?;
    let after = file.metadata()?;
    budget.check()?;
    parent.verify_controlled(&mut files)?;
    if before.len() != after.len() || raw.len() != size || before.modified()? != after.modified()? {
        return Err(identity("Cache changed while reading; retry a stable copy"));
    }
    Ok(raw)
}
fn digest(raw: &[u8], budget: &Budget) -> Result<String> {
    let mut hash = Sha256::new();
    for block in raw.chunks(65536) {
        budget.charge(Cost::work(1))?;
        hash.update(block);
    }
    budget.charge(Cost::retained(64))?;
    Ok(crate::casc::hex(&hash.finalize()))
}
fn number(inv: &Invocation, flag: &str) -> Result<Option<u32>> {
    inv.flag(flag)
        .map(|value| {
            if let Some(hex) = value.strip_prefix("0x") {
                u32::from_str_radix(hex, 16)
            } else {
                value.parse()
            }
        })
        .transpose()
        .map_err(|_| fault("records.hotfix_query", 2, format!("Invalid {flag}")))
}
fn request_limit(inv: &Invocation, flag: &str, default: usize, maximum: usize) -> Result<usize> {
    let value = inv
        .flag(flag)
        .map(str::parse::<usize>)
        .transpose()
        .map_err(|_| ToolkitError::arguments(format!("Invalid {flag}")))?
        .unwrap_or(default);
    if value == 0 || value > maximum {
        return Err(fault(
            "records.hotfix_limit",
            2,
            "Hotfix request exceeds input/page limits",
        ));
    }
    Ok(value)
}
fn validate_source_flags(inv: &Invocation) -> Result<()> {
    for flag in [
        "--raidbots",
        "--product",
        "--build",
        "--region",
        "--locale",
        "--search",
        "--to",
        "--cursor",
    ] {
        if inv.flag(flag).is_some_and(|value| !value.is_empty())
            && !(flag == "--cursor" && inv.flag_bool("--scan"))
        {
            return Err(ToolkitError::arguments(format!(
                "{flag} is not supported by the dbcache hotfix source"
            )));
        }
    }
    for flag in ["--page", "--max-pages", "--max-requests"] {
        if number(inv, flag)?.is_some_and(|value| value != 0)
            && !(flag == "--max-pages" && inv.flag_bool("--scan"))
        {
            return Err(ToolkitError::arguments(format!(
                "{flag} is not supported by the dbcache hotfix source"
            )));
        }
    }
    if inv.flag("--encoding") == Some("csv") {
        return Err(ToolkitError::arguments(
            "--encoding csv is not supported by the dbcache hotfix source",
        ));
    }
    Ok(())
}
fn validate_data(data: &selection::DataPin) -> Result<()> {
    let profile = lycheedev_core::client::product(&data.product).ok_or_else(|| {
        fault(
            "selection.unsupported_data_product",
            2,
            "Unknown data product",
        )
    })?;
    if profile.reused_slot && !data.full_build.starts_with(&format!("{}.", profile.series)) {
        return Err(fault(
            "selection.data_product_build_mismatch",
            4,
            "Unverified reused data slot build",
        ));
    }
    if ![
        "enUS", "koKR", "frFR", "deDE", "zhCN", "esES", "zhTW", "enGB", "esMX", "ruRU", "ptBR",
        "itIT", "ptPT",
    ]
    .contains(&data.language.as_str())
    {
        return Err(fault(
            "selection.unsupported_data_language",
            2,
            "Unknown data language",
        ));
    }
    if !["us", "eu", "cn", "kr", "tw"].contains(&data.region.as_str()) {
        return Err(fault(
            "selection.unsupported_data_region",
            2,
            "Unknown data region",
        ));
    }
    Ok(())
}
fn named_definition(
    inv: &Invocation,
    data: &selection::DataPin,
    name: &str,
    budget: &Budget,
) -> Result<(definitions::Bundle, schema::Definition)> {
    let (bundle, raw) = definitions::prepare_budgeted(
        &inv.home,
        &data.definition_commit,
        name,
        inv.flag_bool("--offline"),
        budget.deadline,
        Some(budget),
    )?;
    budget.check()?;
    let bytes = bundle.manifest["bytes"]
        .as_u64()
        .filter(|bytes| *bytes <= 4 << 20)
        .ok_or_else(|| identity("Invalid definition manifest reference"))? as usize;
    budget.charge(Cost::metadata(bytes))?;
    let _manifest_parser = budget.reserve_retained(bytes * 32 + 4096)?;
    let manifest = evidence::read_blob_bounded(&inv.home, &bundle.manifest, 4 << 20, &inv.control)?;
    let entries: Vec<Value> = serde_json::from_slice(&manifest)?;
    let mut matches = 0;
    for entry in entries {
        budget.check()?;
        if entry["tableHash"]
            .as_str()
            .and_then(|value| u32::from_str_radix(value, 16).ok())
            == Some(bundle.identity.hash)
        {
            matches += 1;
        }
    }
    // XFTH has no file ID: even a requested name cannot disambiguate its hash.
    if matches > 1 {
        return Err(fault(
            "schema.ambiguous_definition",
            4,
            "Multiple manifest names share the selected table hash",
        ));
    }
    if matches != 1 {
        return Err(fault(
            "records.definition_identity",
            4,
            "Definition manifest identity changed",
        ));
    }
    let _definition_parser = budget.reserve_retained(raw.len() * 32 + 4096)?;
    let definition = schema::Document::parse(&raw)?.select(&data.full_build, "")?;
    let owned = size_of::<schema::Definition>()
        + definition.sha256.len()
        + definition.r#match.len()
        + definition.build.len()
        + definition.layout.len()
        + definition
            .fields
            .iter()
            .map(|field| {
                size_of::<schema::Field>()
                    + field.name.len()
                    + field.kind.len()
                    + field.foreign_table.len()
                    + field.foreign_column.len()
            })
            .sum::<usize>();
    budget.charge(Cost::retained(owned))?;
    budget.check()?;
    Ok((bundle, definition))
}

// Input bytes are independently bounded by --max-bytes. Leave the existing
// 128 MiB definition/metadata allowance in addition to that input, plus bounded
// typed-page, escaped JSON and public Value allocations (32 MiB x 16).
fn hotfix_limits(maximum: usize) -> Cost {
    Cost {
        metadata_bytes: maximum as u64 + (128 << 20),
        retained_bytes: maximum as u64 + (1 << 30),
        decode_work: maximum as u64 * 8 + (256 << 20),
        ..Cost::defaults()
    }
}

pub(crate) fn dispatch(inv: &Invocation) -> Result<Outcome> {
    inv.control.check()?;
    let snapshot = inv.required("--snapshot")?;
    if inv
        .flag("--dbcache")
        .filter(|value| !value.is_empty())
        .is_some()
        == inv
            .flag("--from")
            .filter(|value| !value.is_empty())
            .is_some()
    {
        return Err(ToolkitError::arguments(
            "data hotfix --source dbcache requires exactly one of --dbcache or --from",
        ));
    }
    validate_source_flags(inv)?;
    if inv.flag("--table").is_some_and(|value| !value.is_empty())
        && inv.flag("--table-hash").is_some()
    {
        return Err(fault(
            "records.hotfix_identity",
            2,
            "Select --table or --table-hash, not both",
        ));
    }
    if inv.flag("--after-index").is_some()
        && inv
            .flag("--from")
            .filter(|value| !value.is_empty())
            .is_none()
    {
        return Err(fault(
            "records.hotfix_identity",
            2,
            "Pagination requires an archived --from",
        ));
    }
    if inv.flag_bool("--scan") {
        return Err(fault(
            "command.unsupported",
            3,
            "Resumable hotfix scans have not completed Rust parity",
        ));
    }
    let maximum = request_limit(inv, "--max-bytes", 128 << 20, 512 << 20)?;
    let budget = Budget::controlled(
        hotfix_limits(maximum),
        inv.control.deadline(),
        inv.control.clone(),
    );
    budget.check()?;
    let mut filter = CacheFilter {
        build: 0,
        table_hash: number(inv, "--table-hash")?,
        record_id: number(inv, "--record")?,
        push: inv
            .flag("--push")
            .map(str::parse)
            .transpose()
            .map_err(|_| fault("records.hotfix_query", 2, "Invalid signed push"))?,
        region: number(inv, "--region-id")?,
        status: number(inv, "--status")?
            .map(|value| {
                u8::try_from(value)
                    .map_err(|_| fault("records.hotfix_query", 2, "Status outside 0..255"))
            })
            .transpose()?,
        latest: inv.flag_bool("--latest"),
        after_index: number(inv, "--after-index")?,
        limit: request_limit(inv, "--limit", 20, 200)?,
    };
    let mut pin = selection::load_pinned(&inv.home, snapshot)?;
    budget.check()?;
    let data = pin
        .data
        .clone()
        .ok_or_else(|| identity("Hotfix requires pinned data"))?;
    validate_data(&data)?;
    filter.build = schema::version(&data.full_build)?[3];
    let (mut source, raw) = if let Some(id) = inv.flag("--from").filter(|value| !value.is_empty()) {
        let source = evidence::read_capture(&inv.home, id)?;
        budget.check()?;
        if source["provenance"]["kind"] != "hotfix-cache"
            || source["mediaType"] != "application/octet-stream"
            || source["complete"] != true
            || source["truncated"] != false
            || source["provenance"]["dataBuild"] != data.full_build
        {
            return Err(identity(
                "Capture is not a complete cache for the selected build",
            ));
        }
        let origin = selection::load_pinned(
            &inv.home,
            source["provenance"]["snapshot"].as_str().unwrap_or(""),
        )?;
        budget.check()?;
        if origin.data.as_ref() != Some(&data) {
            return Err(identity("Capture data context differs"));
        }
        let bytes = source["blob"]["bytes"]
            .as_u64()
            .filter(|bytes| *bytes <= maximum as u64)
            .ok_or_else(limit)? as usize;
        budget.charge(Cost::metadata(bytes))?;
        let raw = evidence::read_blob_bounded(&inv.home, &source["blob"], maximum, &inv.control)?;
        (Some(source), raw)
    } else {
        (
            None,
            read_file(Path::new(inv.required("--dbcache")?), maximum, &budget)?,
        )
    };
    let named = inv
        .flag("--table")
        .filter(|name| !name.is_empty())
        .map(|name| named_definition(inv, &data, name, &budget))
        .transpose()?;
    if let Some((bundle, _)) = &named {
        filter.table_hash = Some(bundle.identity.hash);
    }
    let mut page = inspect_budgeted(&raw, filter, Some(&budget))?;
    if let Some((_, definition)) = &named {
        page = decode_page(&raw, page, definition, &budget)?;
    }
    let digest = digest(&raw, &budget)?;
    let scope = format!("local-cache:{}", data.language);
    if let Some(changes) = &pin.changes {
        if changes.provider != "dbcache"
            || changes.sha256 != digest
            || changes.product != data.product
            || changes.region != data.region
            || changes.full_build != data.full_build
            || changes.scope != scope
        {
            return Err(identity(
                "Changes pin differs from the immutable local cache",
            ));
        }
    }
    // No cache capture or changes pin is published until framing, selection,
    // definitions and every selected typed payload have all validated.
    if source.is_none() {
        source = Some(evidence::commit_capture_budgeted(
            &inv.home,
            evidence::CaptureInput {
                bytes: &raw,
                maximum_bytes: maximum,
                media_type: "application/octet-stream",
                provenance: json!({"kind":"hotfix-cache", "locator":format!("dbcache:sha256:{digest}"), "snapshot":pin.id, "dataBuild":data.full_build}),
                complete: true,
                truncated: false,
            },
            &inv.control,
            &budget,
        )?);
    }
    let source = source.expect("fresh or verified archived source");
    if pin.changes.is_none() {
        budget.check()?;
        pin = selection::pin_selection_controlled(
            &inv.home,
            selection::SelectionSpec {
                parent: pin.id.clone(),
                changes: Some(selection::ChangePin {
                    provider: "dbcache".into(),
                    product: data.product.clone(),
                    region: data.region.clone(),
                    full_build: data.full_build.clone(),
                    scope,
                    sha256: digest.clone(),
                    fetched_at: source["capturedAt"]
                        .as_str()
                        .ok_or_else(|| identity("Source capture lacks timestamp"))?
                        .into(),
                }),
                ..Default::default()
            },
            &inv.control,
        )?;
    }
    budget.check()?;
    let complete = page.complete;
    let truncated = page.truncated;
    // Serialize bounded output before converting it into the public JSON tree.
    #[derive(serde::Serialize)]
    struct ResultBody<'a> {
        snapshot: &'a str,
        source: &'a Value,
        page: &'a super::CachePage,
        #[serde(skip_serializing_if = "Option::is_none")]
        sources: Option<&'a definitions::Bundle>,
        #[serde(skip_serializing_if = "Option::is_none")]
        definition: Option<&'a schema::Definition>,
    }
    let body = ResultBody {
        snapshot: &pin.id,
        source: &source,
        page: &page,
        sources: named.as_ref().map(|(bundle, _)| bundle),
        definition: named.as_ref().map(|(_, definition)| definition),
    };
    let encoded = capture::json_bytes(&body, &budget, 32 << 20, limit)?;
    budget.charge(Cost::retained(encoded.len().saturating_mul(16)))?;
    let result: Value = serde_json::from_slice(&encoded)?;
    let result_capture = evidence::commit_capture_budgeted(
        &inv.home,
        evidence::CaptureInput {
            bytes: &encoded,
            maximum_bytes: 32 << 20,
            media_type: "application/json",
            provenance: json!({"kind":"hotfix-records", "locator":format!("dbcache:sha256:{digest}"), "snapshot":pin.id, "dataBuild":data.full_build}),
            complete,
            truncated,
        },
        &inv.control,
        &budget,
    )?;
    let mut out = Outcome::new(result);
    out.complete = complete;
    out.selection = serde_json::to_value(pin)?;
    out.provenance = vec![source, result_capture];
    out.coverage = json!({"scope":"selected_cache", "serverCoverage":"unknown"});
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hotfix_ledger_admits_full_input_and_separate_metadata_but_stays_bounded() {
        for maximum in [128 << 20, 512 << 20] {
            let control = lycheedev_core::control::OperationControl::default();
            let budget = Budget::controlled(hotfix_limits(maximum), control.deadline(), control);
            budget.charge(Cost::metadata(maximum)).unwrap();
            // Two maximum definition resources at the existing 8x admission,
            // followed by the remaining metadata/capture allowance.
            budget.charge(Cost::metadata(64 << 20)).unwrap();
            budget.charge(Cost::metadata(64 << 20)).unwrap();
            assert!(
                budget
                    .charge(Cost {
                        metadata_bytes: 1,
                        ..Default::default()
                    })
                    .is_err()
            );
        }
    }
}
