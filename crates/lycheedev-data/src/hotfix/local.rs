//! Fixed Go records.InspectHotfix's local named-table path. Scan checkpoints and
//! remote providers stay separate and explicitly unavailable in this batch.
use super::{LocalInput, LocalRequest, decode_page, inspect_budgeted};
use crate::request::DataContext;
use crate::{
    capture, definitions, fault,
    resource::{Budget, Cost, RetainedReservation},
    schema,
};
use lycheedev_core::{Outcome, Result, ToolkitError, evidence, selection, storage};
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
    control: Option<&'a lycheedev_core::control::OperationControl>,
    temporary: Vec<RetainedReservation<'a>>,
}
impl storage::FileControl for Files<'_> {
    type Error = ToolkitError;
    fn charge(&mut self, cost: storage::FileCost) -> Result<()> {
        if let Some(control) = self.control {
            control.check()?;
        }
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
        if let Some(control) = self.control {
            control.check()?;
        }
        self.budget.check()
    }
}
fn read_file(path: &Path, maximum: usize, budget: &Budget) -> Result<Vec<u8>> {
    read_file_inner(path, maximum, budget, true)
}
fn read_file_inner(
    path: &Path,
    maximum: usize,
    budget: &Budget,
    metadata: bool,
) -> Result<Vec<u8>> {
    read_file_scoped(path, maximum, budget, metadata, None, |_| Ok(()))
}
/// Raidbots is a separately bounded input blob, not JSON metadata.
pub(super) fn read_source_file_controlled(
    path: &Path,
    maximum: usize,
    budget: &Budget,
    control: &lycheedev_core::control::OperationControl,
    validate: impl FnOnce(&std::fs::Metadata) -> Result<()>,
) -> Result<Vec<u8>> {
    read_file_scoped(path, maximum, budget, false, Some(control), validate)
}
fn read_file_scoped(
    path: &Path,
    maximum: usize,
    budget: &Budget,
    metadata: bool,
    control: Option<&lycheedev_core::control::OperationControl>,
    validate: impl FnOnce(&std::fs::Metadata) -> Result<()>,
) -> Result<Vec<u8>> {
    let check = || {
        if let Some(control) = control {
            control.check()?;
        }
        budget.check()
    };
    check()?;
    let mut files = Files {
        budget,
        control,
        temporary: Vec::new(),
    };
    let parent_path = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = storage::Directory::open_controlled(parent_path, &mut files)?;
    let result = (|| {
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
        validate(&observed)?;
        check()?;
        if observed.len() > maximum as u64 {
            return Err(limit());
        }
        let mut file = parent.open_file_controlled(name, &mut files)?;
        let result = (|| {
            budget.charge(Cost::work(1))?;
            let before = file.metadata()?;
            check()?;
            if !before.is_file() || before.file_attributes() & 0x400 != 0 {
                return Err(fault(
                    "records.hotfix_format",
                    4,
                    "Cache must be a regular non-reparse file",
                ));
            }
            if observed.len() != before.len() || observed.modified()? != before.modified()? {
                return Err(identity("Cache changed while opening; retry a stable copy"));
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
                check()?;
                let count = match file.read(&mut block) {
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    result => result?,
                };
                budget.charge(Cost {
                    metadata_bytes: if metadata { count as u64 } else { 0 },
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
            check()?;
            parent.verify_controlled(&mut files)?;
            if before.len() != after.len()
                || raw.len() != size
                || before.modified()? != after.modified()?
            {
                return Err(identity("Cache changed while reading; retry a stable copy"));
            }
            Ok(raw)
        })();
        lycheedev_core::metadata::owned_write::finish_owned(
            result,
            storage::close_file(file).map_err(Into::into),
        )
    })();
    let mut result = result;
    if let Err(errors) = parent.close() {
        for error in errors {
            result = lycheedev_core::metadata::owned_write::finish_owned(result, Err(error.into()));
        }
    }
    if let Some(control) = control {
        result = super::remote::finish_stop(result, control.check());
    }
    let stopped = budget.check();
    if let (Err(primary), Err(stop)) = (&result, &stopped)
        && primary.code == stop.code
        && primary.exit_code == stop.exit_code
        && primary.message == stop.message
        && primary.details == stop.details
    {
        return result;
    }
    lycheedev_core::metadata::owned_write::finish_owned(result, stopped)
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
fn validate_data(data: &selection::DataPin) -> Result<()> {
    let profile = lycheedev_core::client::product(&data.product).ok_or_else(|| {
        fault(
            "selection.unsupported_data_product",
            2,
            "Unknown data product",
        )
    })?;
    if profile.id == "forever" && !data.full_build.starts_with(&format!("{}.", profile.series)) {
        return Err(fault(
            "selection.data_product_build_mismatch",
            4,
            "Pinned build is outside the supported Forever series",
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
    ctx: &DataContext,
    data: &selection::DataPin,
    name: &str,
    budget: &Budget,
    offline: bool,
) -> Result<(definitions::Bundle, schema::Definition)> {
    let (bundle, raw) = definitions::prepare_controlled(
        &ctx.home,
        &data.definition_commit,
        name,
        offline,
        budget.deadline,
        Some(budget),
        &ctx.control,
    )?;
    budget.check()?;
    let bytes = bundle.manifest["bytes"]
        .as_u64()
        .filter(|bytes| *bytes <= 4 << 20)
        .ok_or_else(|| identity("Invalid definition manifest reference"))? as usize;
    budget.charge(Cost::metadata(bytes))?;
    let _manifest_parser = budget.reserve_retained(bytes * 32 + 4096)?;
    let manifest = evidence::read_blob_bounded_with_check(
        &ctx.home,
        &bundle.manifest,
        4 << 20,
        &ctx.control,
        || budget.check(),
    )?;
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
    let definition = schema::Document::parse_with_check(&raw, || {
        ctx.control.check()?;
        budget.check()
    })?
    .select(&data.full_build, "")?;
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

// arbitrary_precision's generic Value parser treats the Go JSON token -0 as
// an integer and loses its sign. Recover only already-decoded field zeros;
// never rewrite strings, integer fields or bytes in the immutable capture.
// Value's public float constructor stores -0.0, which preserves the binary64
// sign in the public result even though its ordinary JSON spelling is longer.
fn restore_field_zero_signs(
    page: &super::CachePage,
    result: &mut Value,
    budget: &Budget,
) -> Result<()> {
    fn mismatch() -> ToolkitError {
        fault("records.internal", 8, "Hotfix result field shape changed")
    }
    fn scalar(original: &Value, result: &mut Value, budget: &Budget) -> Result<()> {
        budget.charge(Cost::work(1))?;
        if original
            .as_f64()
            .is_some_and(|number| number == 0.0 && number.is_sign_negative())
        {
            if result.as_f64() != Some(0.0) {
                return Err(mismatch());
            }
            budget.charge(Cost::retained(4))?;
            *result = Value::from(-0.0);
        }
        Ok(())
    }
    let rows = result
        .get_mut("page")
        .and_then(|page| page.get_mut("entries"))
        .and_then(Value::as_array_mut)
        .filter(|rows| rows.len() == page.entries.len())
        .ok_or_else(mismatch)?;
    for (entry, row) in page.entries.iter().zip(rows) {
        budget.check()?;
        let Some(fields) = &entry.fields else {
            continue;
        };
        let output = row
            .get_mut("fields")
            .and_then(Value::as_object_mut)
            .ok_or_else(mismatch)?;
        for (name, original) in fields {
            budget.charge(Cost::work(name.len().saturating_add(1)))?;
            let output = output.get_mut(name).ok_or_else(mismatch)?;
            if let Value::Array(values) = original {
                let output = output
                    .as_array_mut()
                    .filter(|output| output.len() == values.len())
                    .ok_or_else(mismatch)?;
                for (original, output) in values.iter().zip(output) {
                    scalar(original, output, budget)?;
                }
            } else {
                scalar(original, output, budget)?;
            }
        }
    }
    Ok(())
}

// Attach numeric types while the selected DBD is still available. The public
// encoder never guesses float provenance from a JSON token or field name.
// This one schema layout is shared across all page entries, including rows
// without fields (for example removals), rather than copied once per record.
fn public_number_layout(
    definition: Option<&schema::Definition>,
    budget: &Budget,
) -> Result<lycheedev_core::public_json::NumberLayout> {
    use lycheedev_core::public_json::NumberLayout;
    use std::collections::BTreeMap;
    let Some(definition) = definition else {
        return Ok(NumberLayout::Verbatim);
    };
    let count = definition
        .fields
        .iter()
        .filter(|field| field.kind == "float")
        .count();
    if count == 0 {
        return Ok(NumberLayout::Verbatim);
    }
    // Same BTree allocation bound as typed field decoding, plus three fixed
    // object nodes, the entries Array and optional-fields boxes. Array-valued
    // fields own one box.
    budget.charge(Cost::retained(
        super::tree_bytes::<String, NumberLayout>(count)
            + 3 * super::tree_bytes::<String, NumberLayout>(1)
            + 2 * size_of::<NumberLayout>()
            + "pageentriesfields".len(),
    ))?;
    let mut fields = BTreeMap::new();
    for field in &definition.fields {
        budget.charge(Cost::work(1))?;
        if field.kind != "float" {
            continue;
        }
        budget.charge(Cost::retained(
            field.name.len()
                + if field.array {
                    size_of::<NumberLayout>()
                } else {
                    0
                },
        ))?;
        fields.insert(
            field.name.clone(),
            if field.array {
                NumberLayout::array(NumberLayout::Binary64)
            } else {
                NumberLayout::Binary64
            },
        );
    }
    Ok(NumberLayout::field(
        "page",
        NumberLayout::field(
            "entries",
            NumberLayout::array(NumberLayout::optional_field(
                "fields",
                NumberLayout::Object(fields),
            )),
        ),
    ))
}

pub(crate) fn dispatch(ctx: &DataContext, request: &LocalRequest) -> Result<Outcome> {
    ctx.control.check()?;
    let snapshot = &request.snapshot;
    let maximum = request.max_bytes;
    let budget = Budget::controlled(
        hotfix_limits(maximum),
        ctx.control.deadline(),
        ctx.control.clone(),
    );
    budget.check()?;
    let mut filter = request.filter.clone();
    let mut pin =
        selection::load_pinned_controlled_with_check(&ctx.home, snapshot, &ctx.control, || {
            budget.check()
        })?;
    budget.check()?;
    let data = pin
        .data
        .clone()
        .ok_or_else(|| identity("Hotfix requires pinned data"))?;
    validate_data(&data)?;
    filter.build = schema::version(&data.full_build)?[3];
    let (mut source, raw) = if let LocalInput::Capture(id) = &request.input {
        let source =
            evidence::read_capture_controlled_with_check(&ctx.home, id, &ctx.control, || {
                budget.check()
            })?;
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
        let origin = selection::load_pinned_controlled_with_check(
            &ctx.home,
            source["provenance"]["snapshot"].as_str().unwrap_or(""),
            &ctx.control,
            || budget.check(),
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
        let raw = evidence::read_blob_bounded_with_check(
            &ctx.home,
            &source["blob"],
            maximum,
            &ctx.control,
            || budget.check(),
        )?;
        (Some(source), raw)
    } else {
        (
            None,
            read_file(
                match &request.input {
                    LocalInput::File(path) => path,
                    LocalInput::Capture(_) => unreachable!("archived input handled above"),
                },
                maximum,
                &budget,
            )?,
        )
    };
    let named = request
        .table
        .as_deref()
        .map(|name| named_definition(ctx, &data, name, &budget, request.offline))
        .transpose()?;
    if let Some((bundle, _)) = &named {
        filter.table_hash = Some(bundle.identity.hash);
    }
    let mut page = inspect_budgeted(&raw, filter, Some(&budget))?;
    if let Some((_, definition)) = &named {
        page = decode_page(&raw, page, definition, &budget)?;
    }
    let public_numbers =
        public_number_layout(named.as_ref().map(|(_, definition)| definition), &budget)?;
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
            &ctx.home,
            evidence::CaptureInput {
                bytes: &raw,
                maximum_bytes: maximum,
                media_type: "application/octet-stream",
                provenance: json!({"kind":"hotfix-cache", "locator":format!("dbcache:sha256:{digest}"), "snapshot":pin.id, "dataBuild":data.full_build}),
                complete: true,
                truncated: false,
            },
            &ctx.control,
            &budget,
        )?);
    }
    let source = source.expect("fresh or verified archived source");
    if pin.changes.is_none() {
        budget.check()?;
        pin = selection::pin_selection_controlled_with_check(
            &ctx.home,
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
            &ctx.control,
            || budget.check(),
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
    // Counted fields and archived bytes must use the same Go encoding,
    // including arbitrary-precision Number values. A count-only formatter
    // would admit the right page but still archive different floats (1.0
    // instead of 1). The public Value projection preserves number semantics;
    // its typed public layout below restores Go spelling at the CLI boundary.
    let encoded = capture::json_bytes_with_formatter(
        &body,
        &budget,
        32 << 20,
        limit,
        super::GoFieldsFormatter,
    )?;
    budget.charge(Cost::retained(encoded.len().saturating_mul(16)))?;
    let mut result: Value = serde_json::from_slice(&encoded)?;
    restore_field_zero_signs(&page, &mut result, &budget)?;
    let source_id = source["id"]
        .as_str()
        .ok_or_else(|| identity("Source capture lacks identity"))?;
    let result_capture = evidence::commit_capture_budgeted_with_dependencies(
        &ctx.home,
        evidence::CaptureInput {
            bytes: &encoded,
            maximum_bytes: 32 << 20,
            media_type: "application/json",
            provenance: json!({"kind":"hotfix-records", "locator":format!("dbcache:sha256:{digest}"), "snapshot":pin.id, "dataBuild":data.full_build}),
            complete,
            truncated,
        },
        &[source_id],
        &ctx.control,
        &budget,
    )?;
    let mut out = Outcome::new(result);
    out.public_numbers = public_numbers;
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
    fn formal_forever_hotfix_admission_does_not_relabel_retired_data() {
        let mut data = selection::DataPin {
            product: "forever".into(),
            full_build: "1.60.1.70658".into(),
            region: "cn".into(),
            language: "zhCN".into(),
            ..Default::default()
        };
        validate_data(&data).unwrap();
        for product in ["forever-asia-beta", "forever-cn-beta"] {
            data.product = product.into();
            assert_eq!(
                validate_data(&data).unwrap_err().code,
                "selection.unsupported_data_product"
            );
        }
        data.product = "forever".into();
        data.full_build = "5.5.4.69934".into();
        assert_eq!(
            validate_data(&data).unwrap_err().code,
            "selection.data_product_build_mismatch"
        );
    }
    #[test]
    fn public_layout_comes_from_schema_and_does_not_guess_other_numbers() {
        use lycheedev_core::public_json;
        let definition = schema::Document::parse(
            b"COLUMNS\nfloat F\nfloat A\nint I\n\nBUILD 12.1.0.69875\nF\nA[2]\nI<u64>\n",
        )
        .unwrap()
        .select("12.1.0.69875", "")
        .unwrap();
        let budget = Budget::new(
            Cost::defaults(),
            std::time::Instant::now() + std::time::Duration::from_secs(30),
            None,
        );
        let layout = public_number_layout(Some(&definition), &budget).unwrap();
        let value: Value = serde_json::from_str(r#"{"page":{"entries":[{"fields":{"F":-0.0,"A":[1.0,1e21],"I":18446744073709551615,"unknown":9007199254740993.125}},{"decodeState":"removed"}]},"source":{"raw":-0.0}}"#).unwrap();
        let mut out = Vec::new();
        public_json::write_line(&mut out, &value, &layout).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "{\"page\":{\"entries\":[{\"fields\":{\"A\":[1,1e+21],\"F\":-0,\"I\":18446744073709551615,\"unknown\":9007199254740993.125}},{\"decodeState\":\"removed\"}]},\"source\":{\"raw\":-0.0}}\n"
        );
        assert!(matches!(
            public_number_layout(None, &budget).unwrap(),
            public_json::NumberLayout::Verbatim
        ));
    }

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

#[cfg(test)]
mod control_tests;
