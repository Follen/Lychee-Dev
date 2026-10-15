//! Local pinned CLI adapters. Every successful file is fully CKey checked;
//! Encoding claims cover authenticated framing and requested pages only.
use crate::{
    blte::{self, ByteSource},
    capture, casc, decoded_cache, definitions, fault, installed, keys,
    metadata_format::{self, Encoding},
    relational,
    resource::{Budget, Cost},
    schema, table,
};
use lycheedev_core::{Invocation, Outcome, Result, evidence, selection};
use serde_json::{Value, json};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    io::Read,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

mod effective;
mod query_csv;
#[cfg(test)]
mod regression_tests;
mod request;

pub struct File {
    pub bytes: Vec<u8>,
    pub reading: Value,
}
pub(crate) struct VerifiedFile {
    pub blob: evidence::VerifiedBlob,
    pub reading: Value,
}
#[derive(Clone, Copy, PartialEq)]
enum ReadMode {
    Materialized,
    Verified,
}
enum SelectedContent {
    Bytes(Vec<u8>),
    Blob(evidence::VerifiedBlob),
}
struct ResolvedFile {
    content: SelectedContent,
    reading: Value,
}
fn locale(language: &str) -> Result<u32> {
    match language {
        "enUS" => Ok(2),
        "koKR" => Ok(4),
        "frFR" => Ok(0x10),
        "deDE" => Ok(0x20),
        "zhCN" => Ok(0x40),
        "esES" => Ok(0x80),
        "zhTW" => Ok(0x100),
        "enGB" => Ok(0x200),
        "esMX" => Ok(0x1000),
        "ruRU" => Ok(0x2000),
        "ptBR" => Ok(0x4000),
        "itIT" => Ok(0x8000),
        "ptPT" => Ok(0x10000),
        _ => Err(fault(
            "selection.unsupported_data_language",
            2,
            "Unknown data locale",
        )),
    }
}
pub fn read_files(
    inv: &Invocation,
    pin: &selection::DataPin,
    ids: &[u32],
    deadline: Instant,
) -> Result<Vec<File>> {
    let budget = Arc::new(Budget::controlled(
        Cost::defaults(),
        deadline,
        inv.control.clone(),
    ));
    read_files_budgeted(inv, pin, ids, &budget)
}
pub(crate) fn read_files_budgeted(
    inv: &Invocation,
    pin: &selection::DataPin,
    ids: &[u32],
    budget: &Arc<Budget>,
) -> Result<Vec<File>> {
    let mut files = Vec::new();
    read_file_batches_budgeted(inv, pin, ids, budget, &mut |batch| {
        files.extend(batch);
        Ok(Vec::new())
    })?;
    Ok(files)
}

/// The callback may request another bounded batch while the authenticated
/// Encoding/Root session remains alive. Once a callback has run it is never
/// replayed, including on a missing key or cancellation.
pub(crate) fn read_file_batches_budgeted(
    inv: &Invocation,
    pin: &selection::DataPin,
    ids: &[u32],
    budget: &Arc<Budget>,
    callback: &mut dyn FnMut(Vec<File>) -> Result<Vec<u32>>,
) -> Result<()> {
    read_batches(
        inv,
        pin,
        ids,
        budget,
        ReadMode::Materialized,
        &mut |files| {
            let files = files
                .into_iter()
                .map(|file| match file.content {
                    SelectedContent::Bytes(bytes) => Ok(File {
                        bytes,
                        reading: file.reading,
                    }),
                    SelectedContent::Blob(_) => Err(fault(
                        "records.internal",
                        8,
                        "Unexpected content representation",
                    )),
                })
                .collect::<Result<Vec<_>>>()?;
            callback(files)
        },
    )
}
pub(crate) fn read_verified_file_budgeted(
    inv: &Invocation,
    pin: &selection::DataPin,
    id: u32,
    budget: &Arc<Budget>,
) -> Result<VerifiedFile> {
    let mut result = None;
    read_batches(
        inv,
        pin,
        &[id],
        budget,
        ReadMode::Verified,
        &mut |mut files| {
            let file = files
                .pop()
                .ok_or_else(|| fault("records.internal", 8, "Selected content missing"))?;
            match file.content {
                SelectedContent::Blob(blob) => {
                    result = Some(VerifiedFile {
                        blob,
                        reading: file.reading,
                    })
                }
                SelectedContent::Bytes(_) => {
                    return Err(fault(
                        "records.internal",
                        8,
                        "Unexpected content representation",
                    ));
                }
            }
            Ok(Vec::new())
        },
    )?;
    result.ok_or_else(|| fault("records.internal", 8, "Selected content missing"))
}
fn read_batches(
    inv: &Invocation,
    pin: &selection::DataPin,
    ids: &[u32],
    budget: &Arc<Budget>,
    mode: ReadMode,
    callback: &mut dyn FnMut(Vec<ResolvedFile>) -> Result<Vec<u32>>,
) -> Result<()> {
    budget.check()?;
    let deadline = budget.deadline;
    let mut keys = keys::Context::prepare_budgeted(inv, deadline, Some(budget.clone()))?;
    let mut delivered = false;
    let first = read_files_with_keys(
        inv,
        pin,
        ids,
        deadline,
        &mut keys,
        budget,
        mode,
        &mut |files| {
            delivered = true;
            callback(files)
        },
    );
    match first {
        Err(error)
            if error.code == "container.key_unavailable"
                && !delivered
                && error.details["missingKey"] == true
                && keys.source.kind == "public"
                && keys.source.state != "loaded" =>
        {
            // Only a missing key encountered by the borrowed Encoding reader
            // needs a fresh key set. The content decoder may already have
            // loaded public keys; a missing key after that or a failed provider
            // must retain its original error without replaying the whole chain.
            keys.ensure_public()?;
            read_files_with_keys(inv, pin, ids, deadline, &mut keys, budget, mode, callback)
        }
        result => result,
    }
}
#[allow(clippy::too_many_arguments)]
fn read_files_with_keys(
    inv: &Invocation,
    pin: &selection::DataPin,
    ids: &[u32],
    deadline: Instant,
    keys: &mut keys::Context,
    budget: &Budget,
    mode: ReadMode,
    callback: &mut dyn FnMut(Vec<ResolvedFile>) -> Result<Vec<u32>>,
) -> Result<()> {
    if inv.flag_bool("--cdn") || inv.flag("--installation").is_none() {
        return Err(fault(
            "records.source_unavailable",
            3,
            "This local reader requires explicit --installation; CDN transport is pending parity",
        ));
    }
    if ids.is_empty() || ids.len() > 32 || ids.contains(&0) {
        return Err(fault(
            "records.file_query",
            2,
            "Require 1..32 positive file IDs",
        ));
    }
    let variant = inv.flag("--content-variant").unwrap_or("");
    if !["", "standard", "low-violence"].contains(&variant) {
        return Err(fault("records.file_query", 2, "Invalid content variant"));
    }
    let max = inv.usize_flag("--max-bytes", 128 << 20, 1, 512 << 20)?;
    budget.charge(Cost::metadata(
        inv.required("--installation")?.len()
            + pin.product.len()
            + pin.full_build.len()
            + pin.region.len()
            + pin.language.len()
            + pin.definition_commit.len(),
    ))?;
    let target = installed::resolve_pinned_target_budgeted(
        Path::new(inv.required("--installation")?),
        pin,
        Some(budget),
    )?;
    if &target.data != pin {
        return Err(fault(
            "records.pinned_build_changed",
            4,
            "Local configurations differ from fixed DataPin",
        ));
    }
    let limits = casc::Limits {
        deadline,
        ..Default::default()
    };
    // Encoding range borrows an immutable key set. A missing public key while
    // reading its metadata restarts this read-only session after lazy loading.
    let range_keys = keys.keys().clone();
    let mut stats = decoded_cache::Stats::default();
    let reader = casc::open_local_reader_budgeted(
        &target.common_root,
        &target.metadata.encoding_key,
        target.metadata.encoding_bytes,
        limits,
        budget.token(),
        Some(budget),
    )?;
    let ranges = blte::Ranges::open_reader_budgeted(
        &reader,
        blte::Limits {
            decoded_bytes: target.metadata.content_bytes,
            ..Default::default()
        },
        &range_keys,
        budget,
    )?;
    if ranges.size() != target.metadata.content_bytes {
        return Err(fault(
            "records.invalid_metadata",
            4,
            "Encoding logical size differs from authenticated config",
        ));
    }
    let metered = MetadataSource {
        inner: &ranges,
        budget,
    };
    let index = Encoding::open(&metered, limits, budget.token())?;
    let root_record = index.find(&target.metadata.root_content_key, limits, budget.token())?;
    let (_, root) = decoded_cache::read_content_budgeted(
        &inv.home,
        &target.common_root,
        &index,
        &root_record,
        limits,
        keys,
        &mut stats,
        Some(budget),
        None,
    )?;
    let root_blob = evidence::publish_blob(&inv.home, &root.bytes, 256 << 20)?;
    let build_blob =
        evidence::publish_blob(&inv.home, &target.metadata.build_document.raw, 4 << 20)?;
    let cdn_blob = evidence::publish_blob(&inv.home, &target.metadata.cdn_document.raw, 4 << 20)?;
    let mask = locale(&pin.language)?;
    let mut retained = root.bytes.len();
    let mut pending = ids.to_vec();
    let mut requested = 0usize;
    while !pending.is_empty() {
        budget.check()?;
        requested = requested
            .checked_add(pending.len())
            .ok_or_else(|| fault("records.file_query", 2, "File batch count overflow"))?;
        if requested > 32 || pending.contains(&0) {
            return Err(fault(
                "records.file_query",
                2,
                "Require at most 32 positive file IDs across all batches",
            ));
        }
        let entries = metadata_format::lookup_root(
            &root,
            &pending,
            metadata_format::RootLimits {
                operation: limits,
                ..Default::default()
            },
            budget.token(),
        )?;
        let mut result = vec![];
        for id in &pending {
            budget.check()?;
            if Instant::now() >= deadline {
                return Err(fault(
                    "records.resource_limit",
                    3,
                    "Local query deadline exceeded",
                ));
            }
            let candidates = entries
                .iter()
                .filter(|e| {
                    e.file_data_id == *id
                        && e.locale_mask & mask != 0
                        && (variant != "standard" || e.content_flags & 0x80 == 0)
                        && (variant != "low-violence" || e.content_flags & 0x80 != 0)
                })
                .collect::<Vec<_>>();
            if candidates.len() != 1 {
                return Err(fault(
                    if candidates.is_empty() {
                        "records.content_missing"
                    } else {
                        "records.file_ambiguous"
                    },
                    if candidates.is_empty() { 3 } else { 4 },
                    "Pinned file locale/content variant must select exactly one Root record",
                ));
            }
            let entry = candidates[0];
            let mapping = index.find(&entry.content_key, limits, budget.token())?;
            if mapping.decoded_bytes > max as u64 {
                return Err(fault(
                    "records.metadata_limit",
                    3,
                    "Selected content exceeds byte budget",
                ));
            }
            if mode == ReadMode::Materialized {
                retained = retained
                    .checked_add(mapping.decoded_bytes as usize)
                    .ok_or_else(|| {
                        fault(
                            "records.query_resource_budget",
                            3,
                            "Retained budget overflow",
                        )
                    })?;
                if retained > 512 << 20 {
                    return Err(fault(
                        "records.query_resource_budget",
                        3,
                        "Aggregate retained content budget exceeded",
                    ));
                }
            }
            // Selected encoded and decoded lengths share --max-bytes; metadata
            // keeps its independent allowance. Pass the selected bound separately
            // so a 512 MiB allowance does not alter CASC allocation limits.
            let (payload_key, content, content_blob) = match mode {
                ReadMode::Materialized => {
                    let (object, content) = decoded_cache::read_content_budgeted(
                        &inv.home,
                        &target.common_root,
                        &index,
                        &mapping,
                        limits,
                        keys,
                        &mut stats,
                        Some(budget),
                        Some(max),
                    )?;
                    let blob = evidence::publish_blob(&inv.home, &content.bytes, max)?;
                    (
                        object.encoding_key,
                        SelectedContent::Bytes(content.bytes),
                        blob,
                    )
                }
                ReadMode::Verified => {
                    let (encoding_key, blob) = decoded_cache::read_verified_content(
                        &inv.home,
                        &target.common_root,
                        &index,
                        &mapping,
                        limits,
                        max,
                        keys,
                        &mut stats,
                        budget,
                        &inv.control,
                    )?;
                    let reference = serde_json::to_value(blob.reference())?;
                    (encoding_key, SelectedContent::Blob(blob), reference)
                }
            };
            let mut reading = json!({"contentVerified":true,"source":"installation","pin":pin,"catalogSHA256":target.metadata.catalog_sha256,"buildConfiguration":build_blob,"cdnConfiguration":cdn_blob,"encodingKey":target.metadata.encoding_key,"root":root_blob,"entry":entry,"payloadEncodingKey":payload_key,"content":content_blob,"verification":{"encodingFramingEKey":true,"encodingRequestedPagesMD5":true,"encodingWholeDecodedCKey":false,"rootWholeDecodedCKey":true,"contentWholeDecodedCKey":true},"encodingIO":reader.io_stats()});
            if !variant.is_empty() {
                reading["contentVariant"] = json!(variant);
            }
            reading["keySource"] = keys.value();
            reading["decodedCache"] = serde_json::to_value(&stats)?;
            result.push(ResolvedFile { content, reading });
            stats = decoded_cache::Stats::default();
        }
        budget.check()?;
        pending = callback(result)?;
    }
    let after = installed::resolve_local_budgeted(
        &target.common_root,
        &target.metadata.installed.product,
        &pin.full_build,
        Some(budget),
    )?;
    if after.catalog_sha256 != target.metadata.catalog_sha256
        || after.build_document.sha256 != target.metadata.build_document.sha256
        || after.cdn_document.sha256 != target.metadata.cdn_document.sha256
    {
        return Err(fault(
            "records.pinned_build_changed",
            4,
            "Local metadata changed during query",
        ));
    }
    budget.check()
}
struct MetadataSource<'a> {
    inner: &'a dyn ByteSource,
    budget: &'a Budget,
}
impl ByteSource for MetadataSource<'_> {
    fn size(&self) -> usize {
        self.inner.size()
    }
    fn read_span(&self, offset: usize, bytes: usize) -> Result<Cow<'_, [u8]>> {
        self.budget.charge(Cost {
            metadata_bytes: bytes as u64,
            decode_work: bytes.div_ceil(16) as u64,
            ..Default::default()
        })?;
        self.inner.read_span(offset, bytes)
    }
    fn read_checked_span(
        &self,
        offset: usize,
        bytes: usize,
        expected: &[u8],
    ) -> Result<Cow<'_, [u8]>> {
        self.budget.charge(Cost {
            metadata_bytes: bytes as u64,
            decode_work: bytes.div_ceil(16) as u64,
            ..Default::default()
        })?;
        self.inner.read_checked_span(offset, bytes, expected)
    }
}
fn scalar(value: Value) -> Result<relational::Scalar> {
    Ok(match value {
        Value::Null => relational::Scalar::Null,
        Value::Bool(v) => relational::Scalar::Bool(v),
        Value::String(v) => relational::Scalar::String(v),
        Value::Number(v) => {
            if let Some(v) = v.as_i64() {
                relational::Scalar::Int(v)
            } else if let Some(v) = v.as_u64() {
                relational::Scalar::UInt(v)
            } else if let Some(v) = v.as_f64().filter(|v| v.is_finite()) {
                relational::Scalar::Float(v)
            } else {
                return Err(fault(
                    "relational.numeric_range",
                    4,
                    "Parameter outside finite numeric range",
                ));
            }
        }
        _ => {
            return Err(fault(
                "relational.invalid_binding",
                2,
                "Parameters must be scalars",
            ));
        }
    })
}
// Conservatively account BTreeMap node storage, including sparse one-cell
// row layouts. Charging only a per-item size misses the root's spare slots.
fn number_tree_bytes<K>(entries: usize) -> usize {
    if entries == 0 {
        return 0;
    }
    (entries.div_ceil(5) + 2)
        .saturating_mul(16 * size_of::<(K, lycheedev_core::public_json::NumberLayout)>() + 128)
}

fn public_query_numbers(
    request: &request::Request,
    rows: &[Vec<relational::Scalar>],
    budget: &Budget,
) -> Result<lycheedev_core::public_json::NumberLayout> {
    use lycheedev_core::public_json::NumberLayout;
    let mut numbers = BTreeMap::new();
    for (row_index, row) in rows.iter().enumerate() {
        budget.check()?;
        let mut cells = BTreeMap::new();
        for (column, value) in row.iter().enumerate() {
            budget.charge(Cost::work(1))?;
            if matches!(value, relational::Scalar::Float(_)) {
                budget.charge(Cost::retained(
                    number_tree_bytes::<usize>(cells.len() + 1)
                        - number_tree_bytes::<usize>(cells.len()),
                ))?;
                cells.insert(column, NumberLayout::Binary64);
            }
        }
        if !cells.is_empty() {
            budget.charge(Cost::retained(
                number_tree_bytes::<usize>(numbers.len() + 1)
                    - number_tree_bytes::<usize>(numbers.len()),
            ))?;
            numbers.insert(row_index, NumberLayout::Indexed(cells));
        }
    }
    budget.charge(Cost::retained(
        2 * number_tree_bytes::<String>(3) + number_tree_bytes::<String>(1) + 128,
    ))?;
    Ok(NumberLayout::Object(BTreeMap::from([
        ("query".into(), request.public_numbers(budget)?),
        (
            "result".into(),
            NumberLayout::field("rows", NumberLayout::Indexed(numbers)),
        ),
        (
            "timings".into(),
            NumberLayout::Object(BTreeMap::from([
                ("definitionsMs".into(), NumberLayout::Binary64),
                ("preparationMs".into(), NumberLayout::Binary64),
                ("executionMs".into(), NumberLayout::Binary64),
            ])),
        ),
    ])))
}

pub fn sql(inv: &Invocation) -> Result<Outcome> {
    sql_inner(inv).map_err(|mut error| {
        let mapped = match error.code.as_str() {
            "relational.invalid_syntax" => {
                Some(("query.invalid_syntax", "relational.invalid_syntax", 2))
            }
            "relational.invalid_binding" => Some((
                "query.unresolved_binding",
                "relational.unresolved_binding",
                2,
            )),
            "relational.unsupported" => Some((
                "query.unsupported_expression",
                "relational.unsupported_expression",
                3,
            )),
            "relational.resource_limit" => {
                Some(("query.budget_exceeded", "relational.budget_exceeded", 3))
            }
            "relational.type_mismatch" => {
                Some(("query.type_mismatch", "relational.type_mismatch", 4))
            }
            "relational.numeric_range" => {
                Some(("query.numeric_range", "relational.numeric_range", 4))
            }
            _ => None,
        };
        if let Some((code, message, exit)) = mapped {
            let diagnostic =
                error.details.get("column").is_some() || error.details.get("location").is_some();
            if let Some(details) = error.details.as_object_mut() {
                details.insert("stage".into(), json!("query"));
            } else {
                error.details = json!({"stage":"query"});
            }
            error.code = code.into();
            if !diagnostic {
                error.message = message.into();
            }
            error.exit_code = exit;
        }
        error
    })
}
fn sql_inner(inv: &Invocation) -> Result<Outcome> {
    sql_inner_after_execute(inv, &mut || Ok(()))
}
fn sql_inner_after_execute(
    inv: &Invocation,
    after_execute: &mut impl FnMut() -> Result<()>,
) -> Result<Outcome> {
    let started = Instant::now();
    let timeout = inv.usize_flag("--timeout-seconds", 300, 1, 3600)?;
    let budget = Arc::new(Budget::controlled(
        Cost::defaults(),
        started + Duration::from_secs(timeout as u64),
        inv.control.clone(),
    ));
    let deadline = budget.deadline;
    budget.check()?;
    let csv = inv.flag("--encoding") == Some("csv");
    if (csv && inv.flag("--output").is_none_or(str::is_empty))
        || (!csv
            && (inv.flag("--output").is_some_and(|path| !path.is_empty())
                || inv.flag_bool("--overwrite")))
    {
        return Err(lycheedev_core::ToolkitError::arguments(
            "data sql CSV export requires --encoding csv and --output; --overwrite is CSV-only",
        ));
    }
    // Go validates this flag even when only metadata sources are referenced.
    inv.usize_flag("--max-bytes", 128 << 20, 1, 512 << 20)?;
    let safety = budget.safety_only();
    let request = request::read(inv, &safety)?;
    safety.charge(Cost::metadata(request.sql.len() * 64 + 4096))?;
    let program = relational::Program::compile(&request.sql)?;
    if program.source_tables().len() > 32 {
        return Err(fault(
            "relational.resource_limit",
            3,
            "SQL request exceeds its bound",
        ));
    }
    program.validate_parameters(&request.parameters)?;
    request.admit_encoded(&safety)?;
    let pin = selection::resolve_pinned(inv)?;
    let data = pin.data.as_ref().ok_or_else(|| {
        fault(
            "records.data_pin_required",
            2,
            "SQL requires a fixed DataPin",
        )
    })?;
    // Go Execute canonicalizes parameters before invoking any source resolver.
    // Keep request size/name and pin admission earlier, but no provider error
    // may mask an invalid numeric scalar.
    let parameters = request.bindings()?;
    // Go reserves the executor once before source preparation. Its separate
    // 64 MiB Work limit still bounds Rust execution, whose actual allocations
    // remain charged to the safety ledger rather than charged twice logically.
    budget.charge_split(Cost::retained(64 << 20), Cost::default())?;
    let definitions_started = Instant::now();
    let mut bundles = vec![];
    let mut semantic_sources = BTreeMap::new();
    let mut semantic_provenance = vec![];
    for name in program.source_tables() {
        budget.check()?;
        if let Some((catalog, table)) = name.split_once('.')
            && catalog.eq_ignore_ascii_case("meta")
        {
            let (source, provenance) = definitions::semantics::source(
                &inv.home,
                &data.definition_commit,
                &data.full_build,
                table,
                inv.flag_bool("--offline"),
                &budget,
                &inv.control,
            )?;
            semantic_sources.insert(name.to_owned(), source);
            semantic_provenance.push(provenance);
            continue;
        }
        let canonical = if let Some((catalog, name)) = name.split_once('.') {
            if !catalog.eq_ignore_ascii_case("static") && !catalog.eq_ignore_ascii_case("effective")
            {
                return Err(fault(
                    "relational.unsupported",
                    3,
                    "SQL catalog must be static, effective or meta",
                ));
            }
            name
        } else {
            name
        };
        bundles.push((
            name.to_owned(),
            definitions::prepare_budgeted(
                &inv.home,
                &data.definition_commit,
                canonical,
                inv.flag_bool("--offline"),
                deadline,
                Some(&budget),
            )?,
        ));
    }
    let definitions_ms = definitions_started.elapsed().as_secs_f64() * 1000.0;
    let ids = bundles
        .iter()
        .map(|(_, (bundle, _))| bundle.identity.db2_file_data_id)
        .collect::<Vec<_>>();
    let mut bundles = Some(bundles);
    let mut query_result = None;
    let mut semantic_sources = Some(semantic_sources);
    let mut execute = |files: Vec<File>| -> Result<Vec<u32>> {
        let prepared = Instant::now();
        let mut sources = semantic_sources
            .take()
            .ok_or_else(|| fault("records.internal", 8, "SQL source callback repeated"))?;
        let mut provenance = vec![];
        let mut decoded_bytes = 0;
        let mut snapshots = effective::Snapshots::new(inv, data, &budget);
        let mut effective_sources = vec![];
        for ((name, (bundle, dbd)), file) in bundles
            .take()
            .ok_or_else(|| fault("records.internal", 8, "SQL source callback repeated"))?
            .into_iter()
            .zip(files)
        {
            budget.check()?;
            let layout = table::inspect_budgeted(&file.bytes, Default::default(), budget.clone())?;
            if layout.table_hash != bundle.identity.hash {
                return Err(fault(
                    "schema.invalid_format",
                    4,
                    "WDC table hash differs from manifest identity",
                ));
            }
            let definition = schema::Document::parse(&dbd)?
                .select(&data.full_build, &format!("{:08X}", layout.layout_hash))?;
            let table = table::open_budgeted(
                &file.bytes,
                &definition,
                Default::default(),
                &Default::default(),
                &[],
                budget.clone(),
            )?;
            let limits = relational::Limits {
                deadline,
                ..Default::default()
            };
            let mut source = if program.schema_only() {
                relational::Source::schema_from_wdc_budgeted(
                    &table,
                    limits,
                    budget.token(),
                    Some(&budget),
                )?
            } else {
                relational::Source::from_wdc_budgeted(
                    &table,
                    limits,
                    budget.token(),
                    Some(&budget),
                )?
            };
            if name
                .split_once('.')
                .is_some_and(|(catalog, _)| catalog.eq_ignore_ascii_case("effective"))
            {
                let (overlaid, report) = if program.schema_only() {
                    snapshots.schema(source, &definition, &bundle, &request.hotfix)?
                } else {
                    snapshots.apply(source, &definition, &bundle, &request.hotfix)?
                };
                source = overlaid;
                effective_sources.push(report);
            }
            sources.insert(name, source);
            decoded_bytes += file.bytes.len();
            provenance.push(json!({"complete":table.coverage()["complete"],"truncated":false,"file":file.reading,"sources":bundle,"layoutHash":format!("{:08X}",layout.layout_hash),"schema":definition}));
        }
        let preparation_ms = prepared.elapsed().as_secs_f64() * 1000.0;
        let execution = Instant::now();
        let result = program.execute_budgeted(
            &sources,
            &parameters,
            relational::Limits {
                deadline,
                work: 10_000_000,
                ..Default::default()
            },
            budget.token(),
            Some(&safety),
        )?;
        after_execute()?;
        query_result = Some((
            result,
            provenance,
            decoded_bytes,
            preparation_ms,
            execution.elapsed().as_secs_f64() * 1000.0,
            effective_sources,
        ));
        Ok(Vec::new())
    };
    // Run preparation/SQL inside the authenticated read session. Its final
    // source check is after execution and before publishing any query capture.
    // A lexical CTE can have no physical source and needs no CASC admission.
    if ids.is_empty() {
        execute(Vec::new())?;
    } else {
        read_file_batches_budgeted(inv, data, &ids, &budget, &mut execute)?;
    }
    let (result, provenance, decoded_bytes, preparation_ms, execution_ms, effective_sources) =
        query_result.ok_or_else(|| fault("records.internal", 8, "SQL result missing"))?;
    let complete = result.complete;
    let resources = budget.logical_snapshot()?;
    let public_numbers = public_query_numbers(&request, &result.rows, &safety)?;
    budget.check()?;
    let mut reading = json!({"resources":resources,"complete":complete,"partial":!complete,"query":request,"result":result.public(),"sources":provenance,"timings":{"definitionsMs":definitions_ms,"preparationMs":preparation_ms,"executionMs":execution_ms,"decodedSourceBytes":decoded_bytes},"resourceAccounting":{"scope":"admitted allocation and work across local CASC/Encoding/decoded cache/definitions/WDC/SQL","strategy":"conservative cumulative admission","wholeQueryAccountingComplete":false,"logicalProviderParityComplete":ids.is_empty(),"used":budget.snapshot()?,"remainingGaps":["physical/effective provider logical counters not audited against Go 3.2","SQL plan/order-container transient allocations","uninterruptible single codec call"]}});
    if !semantic_provenance.is_empty() {
        let size = capture::json_size(&semantic_provenance, &safety)?;
        safety.charge(Cost::metadata(size.saturating_mul(4).saturating_add(4096)))?;
        reading["semanticSources"] = serde_json::to_value(&semantic_provenance)?;
    }
    if !effective_sources.is_empty() {
        reading["effectiveSources"] = effective::reports_value(&effective_sources, &safety)?;
    }
    let raw = capture::json_bytes(
        &request::CaptureBody {
            reading: &reading,
            request: &request,
        },
        &safety,
        16 << 20,
        || {
            fault(
                "relational.resource_limit",
                3,
                "SQL result exceeds evidence budget",
            )
        },
    )?;
    let request_digest = request.digest(&safety)?;
    let required_sources = if effective_sources.is_empty() {
        vec![]
    } else {
        request
            .hotfix
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    };
    let reference_accounting = effective::ReferenceAccounting::new(&safety, &inv.control);
    let capture = evidence::commit_capture_budgeted_referencing(
        &inv.home,
        evidence::CaptureInput {
            bytes: &raw,
            maximum_bytes: 16 << 20,
            media_type: "application/json",
            provenance: json!({"kind":"data-query","locator":format!("sql:sha256:{request_digest}"),"snapshot":pin.id,"dataBuild":data.full_build}),
            complete,
            truncated: false,
        },
        &required_sources,
        &inv.control,
        &reference_accounting,
    )?;
    if csv {
        let (manifest, csv_capture) = query_csv::export(inv, &pin.id, &capture, &result, &safety)?;
        let mut out = Outcome::new(manifest);
        out.complete = complete;
        out.selection = serde_json::to_value(&pin)?;
        out.provenance = vec![capture, csv_capture];
        return Ok(out);
    }
    let mut out = Outcome::new(reading);
    out.public_numbers = public_numbers;
    out.complete = complete;
    out.selection = serde_json::to_value(&pin)?;
    out.provenance = vec![capture];
    Ok(out)
}
pub fn asset(inv: &Invocation) -> Result<Outcome> {
    inv.control.check()?;
    let pin = selection::resolve_pinned(inv)?;
    let data = pin.data.as_ref().ok_or_else(|| {
        fault(
            "records.data_pin_required",
            2,
            "Asset requires fixed data pin",
        )
    })?;
    let id = inv.required("--file-id")?.parse::<u32>().map_err(|_| {
        fault(
            "command.invalid_arguments",
            2,
            "File ID must be a positive u32",
        )
    })?;
    let budget = Arc::new(Budget::controlled(
        Cost::defaults(),
        inv.control.deadline(),
        inv.control.clone(),
    ));
    let mut file = read_verified_file_budgeted(inv, data, id, &budget)?;
    file.reading["resourceAccounting"] =
        json!({"wholeQueryAccountingComplete":false,"used":budget.snapshot()?});
    budget.check()?;
    let capture = evidence::commit_verified_capture_budgeted(
        &file.blob,
        evidence::CaptureHeader {
            media_type: "application/octet-stream",
            provenance: json!({"kind":"asset","locator":format!("casc:fdid:{id}:ckey:{}",file.reading["entry"]["contentKey"].as_str().unwrap_or("")),"snapshot":pin.id,"dataBuild":data.full_build}),
            complete: true,
            truncated: false,
        },
        &inv.control,
        budget.as_ref(),
    )?;
    let mut out = Outcome::new(file.reading);
    out.selection = serde_json::to_value(pin)?;
    out.provenance = vec![capture];
    Ok(out)
}

#[cfg(test)]
mod request_identity_tests {
    use super::*;

    #[test]
    fn sql_public_number_layout_accounts_sparse_tree_nodes_before_allocation() {
        let request = request::Request {
            sql: "SELECT 1.0 FROM T".into(),
            hotfix: vec![],
            parameters: BTreeMap::new(),
        };
        let rows = vec![vec![relational::Scalar::Float(1.0)]];
        let budget = Budget::new(
            Cost {
                retained_bytes: (number_tree_bytes::<usize>(1) - 1) as u64,
                ..Cost::defaults()
            },
            Instant::now() + Duration::from_secs(30),
            None,
        );
        assert_eq!(
            public_query_numbers(&request, &rows, &budget)
                .unwrap_err()
                .code,
            "records.query_resource_budget"
        );
        assert_eq!(
            budget.snapshot().unwrap().retained_bytes,
            0,
            "Rejected tree admission is atomic"
        );
    }

    #[test]
    fn sql_locator_matches_go_order_omission_html_unicode_and_exact_integer_goldens() {
        use relational::Scalar;
        let cases = [
            (
                "SELECT ID FROM Item WHERE ID=7",
                BTreeMap::new(),
                "c5462d945c5082649dfcab1450f01d37271b4e9f09481a7bd7b63525e22cf9e4",
            ),
            (
                "SELECT ID FROM Item WHERE ID=:id",
                BTreeMap::from([("id".into(), Scalar::Int(7))]),
                "ac2dfa77446d4d737857d235e789eeddcdcdeddc18c5ba419e081d7a9b1df1bc",
            ),
            (
                "SELECT '<>&\u{2028}\u{2029}' FROM Item",
                BTreeMap::from([
                    ("max".into(), Scalar::UInt(u64::MAX)),
                    ("min".into(), Scalar::Int(i64::MIN)),
                    ("text".into(), Scalar::String("<>&\u{2028}\u{2029}".into())),
                ]),
                "3e92190db22f342688a0367540c7cd9e74486c3e28806629691a2024fabe7e76",
            ),
        ];
        for (sql, parameters, expected) in cases {
            let budget = Budget::new(
                Cost {
                    retained_bytes: 64,
                    ..Cost::defaults()
                },
                Instant::now() + Duration::from_secs(30),
                None,
            );
            let request = request::Request {
                sql: sql.into(),
                hotfix: vec![],
                parameters: parameters
                    .into_iter()
                    .map(|(name, value)| {
                        (
                            name,
                            request::Parameter::Other(serde_json::to_value(value).unwrap()),
                        )
                    })
                    .collect(),
            };
            assert_eq!(request.digest(&budget).unwrap(), expected);
            let used = budget.snapshot().unwrap();
            assert_eq!(
                used.retained_bytes, 64,
                "only the returned hex digest is retained"
            );
            assert_eq!(used.metadata_bytes, 0);
            assert!(used.decode_work > 0);
        }
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;

    #[test]
    #[ignore = "actual local encrypted CASC fixture and authenticated pinned public-key document"]
    fn loaded_public_missing_key_does_not_replay_metadata_chain() {
        let root = std::path::PathBuf::from(
            std::env::var_os("LYCHEEDEV_LOCAL_KEY_FIXTURE").expect("prepared owned fixture"),
        );
        let identity: Value =
            serde_json::from_slice(&std::fs::read(root.join("input-identity.json")).unwrap())
                .unwrap();
        let home = std::path::PathBuf::from(identity["home"].as_str().unwrap());
        let pin = selection::load_pinned(&home, identity["snapshot"].as_str().unwrap())
            .unwrap()
            .spec
            .data
            .unwrap();
        let mut flags = BTreeMap::from([
            ("--offline".into(), "true".into()),
            (
                "--installation".into(),
                identity["installation"].as_str().unwrap().into(),
            ),
        ]);
        let mut observed = vec![];
        for source in ["public", "file"] {
            if source == "file" {
                flags.insert(
                    "--key-file".into(),
                    root.join("public-keys.txt").to_string_lossy().into(),
                );
            }
            let inv = Invocation {
                command: "asset inspect".into(),
                positional: None,
                flags: flags.clone(),
                home: home.clone(),
                cwd: root.clone(),
                control: Default::default(),
            };
            let budget = Arc::new(Budget::controlled(
                Cost::defaults(),
                Instant::now() + Duration::from_secs(30),
                inv.control.clone(),
            ));
            let error = read_files_budgeted(&inv, &pin, &[303], &budget)
                .err()
                .expect("actual encrypted chunk's absent key");
            assert_eq!(error.code, "container.key_unavailable");
            assert_eq!(error.details["missingKey"], true);
            assert_eq!(error.details["keyId"], "000000000000002a");
            let cost = budget.snapshot().unwrap();
            assert_eq!(cost.network_requests, 0);
            assert_eq!(cost.network_bytes, 0);
            observed.push((source, cost));
        }
        // Both calls authenticate exactly the same installed bytes and public
        // key document. Lazy content key loading may repeat content decoding,
        // but must not repeat permanently charged Encoding/Root metadata reads.
        assert_eq!(observed[0].1.metadata_bytes, observed[1].1.metadata_bytes);
        assert_eq!(observed[0].1.retained_bytes, observed[1].1.retained_bytes);
        assert!(observed[0].1.decode_work >= observed[1].1.decode_work);
        std::fs::write(
            root.join("retry-report.json"),
            serde_json::to_vec_pretty(&json!({
                "passed":true,
                "scope":"real local CASC/Encoding/Root/encrypted object with authenticated pinned public document; public lazy versus same explicit document; not Go/network/domain acceptance",
                "inputIdentity":identity,
                "publicKeyDocumentSHA256":keys::PUBLIC_SHA256,
                "observations":observed,
                "error":{"code":"container.key_unavailable","missingKey":true,"keyId":"000000000000002a"}
            })).unwrap(),
        ).unwrap();
    }
}
