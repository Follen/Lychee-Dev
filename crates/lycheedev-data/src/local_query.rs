//! Local pinned CLI adapters. Raw files are fully CKey checked; tables may
//! explicitly retain authenticated missing-key spans with partial provenance.
//! Encoding claims cover authenticated framing and requested pages only.
use crate::{
    blte::{self, ByteSource},
    capture, casc, decoded_cache, definitions, fault,
    hotfix::remote::equal_fold,
    installed, keys,
    metadata_format::{self, Encoding},
    phase_profile::Phase,
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

#[cfg(test)]
mod adaptive_tests;
mod effective;
mod file_bytes;
mod provider;
mod resolver;
pub(crate) mod sql_reader;
pub use file_bytes::FileBytes;
pub(crate) mod query_csv;
#[cfg(test)]
mod regression_tests;
mod request;

pub struct File {
    pub bytes: FileBytes,
    pub reading: Value,
}
pub(crate) struct VerifiedFile {
    pub blob: evidence::VerifiedBlob,
    pub reading: Value,
}
#[derive(Clone, Copy, PartialEq)]
enum ReadMode {
    Materialized(blte::DecodePolicy),
    Verified,
}
impl ReadMode {
    fn policy(self) -> blte::DecodePolicy {
        match self {
            Self::Materialized(policy) => policy,
            Self::Verified => blte::DecodePolicy::Strict,
        }
    }
}
enum SelectedContent {
    Bytes(blte::Content),
    Blob(evidence::VerifiedBlob),
}
struct ResolvedFile {
    content: SelectedContent,
    reading: Value,
}
pub(crate) fn report_partial(reading: &mut Value, missing: &[blte::Missing]) -> Result<()> {
    if !missing.is_empty() {
        reading["contentVerified"] = json!(false);
        reading["verification"]["contentWholeDecodedCKey"] = json!(false);
        reading["partialContent"] = reading["content"].take();
        reading["content"] = json!({"sha256":"", "bytes":0});
        reading["missing"] = serde_json::to_value(missing)?;
    }
    Ok(())
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
    let budget = Arc::new(Budget::query_controlled(deadline, inv.control.clone()));
    // Reader FileQuery errors precede admission; DataIdentity deliberately does
    // not. Preserve original control before query checks or workspace IO.
    inv.control.check()?;
    budget.check()?;
    validate_local_file_request(inv, ids)?;
    let scope = {
        let _phase = budget.phase(Phase::Admission);
        crate::remote::provider::OpenedScope::open(inv, &budget)?.admit()?
    };
    let result = (|| {
        scope.check_pin(pin)?;
        read_files_budgeted(inv, pin, ids, &budget)
    })();
    let _phase = budget.phase(Phase::SourceClose);
    scope.finish(result)
}
/// No IO, allocation admission, key preparation or pin lookup. Keep Go's
/// variant-before-FileQuery order with the existing local multi-ID contract.
fn validate_local_file_request(inv: &Invocation, ids: &[u32]) -> Result<usize> {
    let variant = inv.flag("--content-variant").unwrap_or("");
    if !["", "standard", "low-violence"].contains(&variant) {
        return Err(fault("records.file_query", 2, "Invalid content variant"));
    }
    if inv.flag_bool("--cdn") || inv.flag("--installation").is_none_or(str::is_empty) {
        return Err(fault(
            "records.source_unavailable",
            3,
            "This local reader requires explicit --installation",
        ));
    }
    if ids.is_empty() || ids.len() > 32 || ids.contains(&0) {
        return Err(fault(
            "records.file_query",
            2,
            "Require 1..32 positive file IDs",
        ));
    }
    inv.usize_flag("--max-bytes", 128 << 20, 1, 512 << 20)
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
    read_materialized_batches(inv, pin, ids, budget, blte::DecodePolicy::Strict, callback)
}
pub(crate) fn read_table_batches_budgeted(
    inv: &Invocation,
    pin: &selection::DataPin,
    ids: &[u32],
    budget: &Arc<Budget>,
    callback: &mut dyn FnMut(Vec<File>) -> Result<Vec<u32>>,
) -> Result<()> {
    read_materialized_batches(
        inv,
        pin,
        ids,
        budget,
        blte::DecodePolicy::TableAllowMissing,
        callback,
    )
}
fn read_materialized_batches(
    inv: &Invocation,
    pin: &selection::DataPin,
    ids: &[u32],
    budget: &Arc<Budget>,
    policy: blte::DecodePolicy,
    callback: &mut dyn FnMut(Vec<File>) -> Result<Vec<u32>>,
) -> Result<()> {
    read_batches(
        inv,
        pin,
        ids,
        budget,
        ReadMode::Materialized(policy),
        &mut |files| {
            let files = files
                .into_iter()
                .map(|file| match file.content {
                    SelectedContent::Bytes(bytes) => Ok(File {
                        bytes: bytes.into(),
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
    read_verified_file_after_read(inv, pin, id, budget, &mut || Ok(()), &mut |result| result)
}
// Observe only after ownership delivery and after the real batch/reader close.
// Production observers are identities; the actual read and close cannot be skipped.
fn read_verified_file_after_read(
    inv: &Invocation,
    pin: &selection::DataPin,
    id: u32,
    budget: &Arc<Budget>,
    after_delivery: &mut impl FnMut() -> Result<()>,
    after_read: &mut impl FnMut(Result<()>) -> Result<()>,
) -> Result<VerifiedFile> {
    let mut result = None;
    let read = read_batches(
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
            after_delivery()?;
            Ok(Vec::new())
        },
    );
    match after_read(read) {
        Ok(()) => result.ok_or_else(|| fault("records.internal", 8, "Selected content missing")),
        Err(error) => match result {
            Some(file) => {
                let _phase = budget.phase(Phase::SourceClose);
                file.blob.close_after(Err(error))
            }
            None => Err(error),
        },
    }
}
fn read_batches(
    inv: &Invocation,
    pin: &selection::DataPin,
    ids: &[u32],
    budget: &Arc<Budget>,
    mode: ReadMode,
    callback: &mut dyn FnMut(Vec<ResolvedFile>) -> Result<Vec<u32>>,
) -> Result<()> {
    inv.control.check()?;
    budget.check()?;
    validate_local_file_request(inv, ids)?;
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
            mode.policy().load_public(&mut keys)?;
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
    let prepare = budget.phase(Phase::Prepare);
    let max = validate_local_file_request(inv, ids)?;
    let variant = inv.flag("--content-variant").unwrap_or("");
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
    let reader = casc::open_local_reader_controlled(
        &target.common_root,
        &target.metadata.encoding_key,
        target.metadata.encoding_bytes,
        limits,
        &inv.control,
        Some(budget),
    )?;
    // All borrowed ranges and metadata readers die before the consuming close.
    let result = (|| {
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
        let check = || {
            inv.control.check()?;
            budget.check()
        };
        let index = Encoding::open_with_check(&metered, limits, None, check)?;
        let root_record =
            index.find_with_check(&target.metadata.root_content_key, limits, None, check)?;
        let (_, root, root_reference) = decoded_cache::read_content_with_reference_controlled(
            &inv.home,
            &target.common_root,
            &index,
            &root_record,
            limits,
            keys,
            &mut stats,
            budget,
            None,
            &inv.control,
        )?;
        // The bounded materialization returned the reference of its actual
        // verified owner only after its original checked close succeeded.
        // Republishing these same Root bytes would re-lock/reopen/re-hash them.
        let root_blob = serde_json::to_value(root_reference)?;
        let build_blob = evidence::publish_blob_bounded_with_check(
            &inv.home,
            &target.metadata.build_document.raw,
            4 << 20,
            &inv.control,
            || budget.check(),
        )?;
        let cdn_blob = evidence::publish_blob_bounded_with_check(
            &inv.home,
            &target.metadata.cdn_document.raw,
            4 << 20,
            &inv.control,
            || budget.check(),
        )?;
        let mask = locale(&pin.language)?;
        let mut pending = ids.to_vec();
        let mut requested = 0usize;
        drop(prepare);
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
            let entries = metadata_format::lookup_root_with_check(
                &root,
                &pending,
                metadata_format::RootLimits {
                    operation: limits,
                    ..Default::default()
                },
                None,
                check,
            )?;
            let mut result = vec![];
            let collected = (|| {
                for id in &pending {
                    let _phase = budget.phase(Phase::Payload);
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
                    let mapping = index.find_with_check(&entry.content_key, limits, None, check)?;
                    if mapping.decoded_bytes > max as u64 {
                        return Err(fault(
                            "records.metadata_limit",
                            3,
                            "Selected content exceeds byte budget",
                        ));
                    }
                    // Go 3.2.3 removed the cumulative raw-content cap. Each file
                    // retains its explicit maximum; shared logical/safety accounting
                    // admits real allocations and may grow only after a fresh probe.
                    // Selected encoded and decoded lengths share --max-bytes; metadata
                    // keeps its independent allowance. Pass the selected bound separately
                    // so a 512 MiB allowance does not alter CASC allocation limits.
                    let (payload_key, content, content_blob) = match mode {
                        ReadMode::Materialized(policy) => {
                            let (encoding_key, content) = decoded_cache::read_content_controlled(
                                &inv.home,
                                &target.common_root,
                                &index,
                                &mapping,
                                limits,
                                keys,
                                &mut stats,
                                budget,
                                Some(max),
                                &inv.control,
                                policy,
                            )?;
                            let blob = evidence::publish_blob_bounded_with_check(
                                &inv.home,
                                &content.bytes,
                                max,
                                &inv.control,
                                || budget.check(),
                            )?;
                            (encoding_key, SelectedContent::Bytes(content), blob)
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
                            let reference = match serde_json::to_value(blob.reference()) {
                                Ok(reference) => reference,
                                Err(error) => {
                                    let _phase = budget.phase(Phase::SourceClose);
                                    return blob.close_after(Err(error.into()));
                                }
                            };
                            (encoding_key, SelectedContent::Blob(blob), reference)
                        }
                    };
                    // Own verified bodies before any fallible report step. Keep
                    // materialized bytes on their original move/drop path.
                    // A partial batch never escapes; verified owners close in
                    // acquisition order if a later file/report/check fails.
                    let content = match content {
                        SelectedContent::Blob(blob) => {
                            result.push(ResolvedFile {
                                content: SelectedContent::Blob(blob),
                                reading: Value::Null,
                            });
                            None
                        }
                        bytes => Some(bytes),
                    };
                    let mut reading = json!({"contentVerified":true,"source":"installation","pin":pin,"catalogSHA256":target.metadata.catalog_sha256,"buildConfiguration":build_blob,"cdnConfiguration":cdn_blob,"encodingKey":target.metadata.encoding_key,"root":root_blob,"entry":entry,"payloadEncodingKey":payload_key,"content":content_blob,"verification":{"encodingFramingEKey":true,"encodingRequestedPagesMD5":true,"encodingWholeDecodedCKey":false,"rootWholeDecodedCKey":true,"contentWholeDecodedCKey":true},"encodingIO":reader.io_stats()});
                    if let Some(SelectedContent::Bytes(bytes)) = &content {
                        report_partial(&mut reading, &bytes.missing)?;
                    }
                    if !variant.is_empty() {
                        reading["contentVariant"] = json!(variant);
                    }
                    reading["keySource"] = keys.value();
                    reading["decodedCache"] = serde_json::to_value(&stats)?;
                    if let Some(content) = content {
                        result.push(ResolvedFile { content, reading });
                    } else {
                        result.last_mut().expect("owned selected file").reading = reading;
                    }
                    stats = decoded_cache::Stats::default();
                }
                Ok(())
            })();
            if let Err(error) = collected {
                return close_resolved_files(result, Err(error), budget);
            }
            pending = deliver_resolved_files(result, budget, callback)?;
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
    })();
    let _phase = budget.phase(Phase::SourceClose);
    reader.close_after(result)
}
// Callback transfer happens only after the original final budget check. On a
// failed batch/check, retain its first typed error and every actual close cause.
fn close_resolved_files<T>(
    files: Vec<ResolvedFile>,
    mut result: Result<T>,
    budget: &Budget,
) -> Result<T> {
    for file in files {
        if let SelectedContent::Blob(blob) = file.content {
            let _phase = budget.phase(Phase::SourceClose);
            result = blob.close_after(result);
        }
    }
    result
}
fn deliver_resolved_files(
    files: Vec<ResolvedFile>,
    budget: &Budget,
    callback: &mut dyn FnMut(Vec<ResolvedFile>) -> Result<Vec<u32>>,
) -> Result<Vec<u32>> {
    match budget.check() {
        Ok(()) => callback(files),
        Err(error) => close_resolved_files(files, Err(error), budget),
    }
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
    result: &relational::QueryResult,
    budget: &Budget,
) -> Result<lycheedev_core::public_json::NumberLayout> {
    use lycheedev_core::public_json::NumberLayout;
    // QueryResult's typed plan branch publishes null columns/rows. Neither
    // EXPLAIN variant has a public row array to receive cell provenance.
    let rows = if result.plan.is_none() {
        result.rows.as_slice()
    } else {
        &[]
    };
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
            if result.plan.is_some() {
                NumberLayout::Verbatim
            } else {
                NumberLayout::field("rows", NumberLayout::Indexed(numbers))
            },
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
            "relational.invalid_cardinality" => Some((
                "query.invalid_cardinality",
                "relational.invalid_cardinality",
                4,
            )),
            _ => None,
        };
        if let Some((code, message, exit)) = mapped {
            let diagnostic = error.code == "relational.invalid_syntax"
                || error.details.get("column").is_some()
                || error.details.get("location").is_some();
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
pub(crate) fn sql_inner_after_execute(
    inv: &Invocation,
    after_execute: &mut impl FnMut() -> Result<()>,
) -> Result<Outcome> {
    let started = Instant::now();
    let timeout = inv.usize_flag("--timeout-seconds", 300, 1, 3600)?;
    let budget = Arc::new(Budget::query_controlled(
        started + Duration::from_secs(timeout as u64),
        inv.control.clone(),
    ));
    sql_inner_budgeted(inv, &budget, after_execute)
}
/// Keep explicit fixture budgets fixed while exercising the same consumer,
/// admission, source checks, execution and capture as the default constructor.
pub(crate) fn sql_inner_budgeted(
    inv: &Invocation,
    budget: &Arc<Budget>,
    after_execute: &mut impl FnMut() -> Result<()>,
) -> Result<Outcome> {
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
    let maximum = inv.usize_flag("--max-bytes", 128 << 20, 1, 512 << 20)?;
    let safety = budget.safety_only();
    let request = request::read(inv, &safety)?;
    safety.charge(Cost::metadata(request.sql.len() * 64 + 4096))?;
    let program = relational::Program::compile_controlled(&request.sql, &inv.control, &safety)?;
    if program.source_tables().len() > 32 {
        return Err(fault(
            "relational.resource_limit",
            3,
            "SQL request exceeds its bound",
        ));
    }
    program.validate_parameters(&request.parameters)?;
    request.admit_encoded(&safety)?;
    // Ordinary SQL keeps Go's Metadata -> pin -> admission ordering for
    // both physical sources. Only file IO branches on --cdn.
    let (mut scope, pin) = {
        let _phase = budget.phase(Phase::Admission);
        crate::remote::provider::OpenedScope::open(inv, budget)?.admit_pinned(fault(
            "records.data_pin_required",
            2,
            "SQL requires a fixed DataPin",
        ))?
    };
    let mut committed = Vec::new();
    let mut output_published = false;
    let result = scope.with_capture_metadata(|scope, metadata| {
        let mut execute_sql = |scope: &mut dyn crate::remote::provider::FileScope| -> Result<Outcome> {
            let data = pin.data.as_ref().ok_or_else(|| {
                fault(
                    "records.data_pin_required",
                    2,
                    "SQL requires a fixed DataPin",
                )
            })?;
            // Go reserves the executor once before source preparation. Its separate
            // 64 MiB Work limit still bounds Rust execution, whose actual allocations
            // remain charged to the safety ledger rather than charged twice logically.
            budget.charge_split(Cost::retained(64 << 20), Cost::default())?;
            // Canonical scalar errors still precede every source resolver.
            let parameters = request.bindings()?;
            let resolver = resolver::Resolver::new(inv, data, budget, scope, maximum, &request.hotfix);
            let resolved = (|| {
                let execution = Instant::now();
                let result = program.execute_resolved_budgeted(
                    &resolver,
                    &parameters,
                    relational::Limits {
                        deadline,
                        work: 10_000_000,
                        ..Default::default()
                    },
                    // Work preserves the original ordered control through this Budget.
                    None,
                    Some(&safety),
                )?;
                after_execute()?;
                resolver.check_source()?;
                let resolver::Report {
                    provenance,
                    semantic: semantic_provenance,
                    effective: effective_sources,
                    decoded_bytes,
                    definitions_ms,
                    preparation_ms,
                } = resolver.take_report()?;
                let execution_ms =
                    (execution.elapsed().as_secs_f64() * 1000.0 - definitions_ms - preparation_ms)
                        .max(0.0);
                let physical_empty = provenance.is_empty();
                let complete = result.complete;
                let resources = budget.logical_snapshot()?;
                let memory_budget = budget.memory_snapshot()?;
                let public_numbers = public_query_numbers(&request, &result, &safety)?;
                budget.check()?;
                let mut reading = json!({"resources":resources,"memoryBudget":memory_budget,"complete":complete,"partial":!complete,"query":request,"result":result.public(),"sources":provenance,"timings":{"definitionsMs":definitions_ms,"preparationMs":preparation_ms,"executionMs":execution_ms,"decodedSourceBytes":decoded_bytes},"resourceAccounting":{"scope":"admitted allocation and work across local CASC/Encoding/decoded cache/definitions/WDC/SQL","strategy":"conservative cumulative admission","wholeQueryAccountingComplete":false,"logicalProviderParityComplete":physical_empty,"used":budget.snapshot()?,"remainingGaps":["physical/effective provider logical counters not audited against Go 3.2","SQL plan/order-container transient allocations","uninterruptible single codec call"]}});
                if !semantic_provenance.is_empty() {
                    let size = capture::json_size(&semantic_provenance, &safety)?;
                    safety.charge(Cost::metadata(size.saturating_mul(4).saturating_add(4096)))?;
                    reading["semanticSources"] = serde_json::to_value(&semantic_provenance)?;
                }
                if !effective_sources.is_empty() {
                    reading["effectiveSources"] =
                        effective::reports_value(&effective_sources, &safety)?;
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
                // Reserve the small success identities before the commit; later
                // checked close cannot erase an already-published capture.
                safety.charge(Cost::retained(512))?;
                let capture_phase = budget.phase(Phase::SourceCapture);
                let capture = evidence::commit_capture_budgeted_referencing_with_metadata(
                    &inv.home,
                    metadata,
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
                drop(capture_phase);
                committed.push(
                    capture["id"]
                        .as_str()
                        .expect("committed SQL capture identity")
                        .to_owned(),
                );
                if csv {
                    let (manifest, csv_capture) =
                        query_csv::export_with_metadata(inv, metadata, &pin.id, &capture, &result, &safety)?;
                    committed.push(
                        csv_capture["id"]
                            .as_str()
                            .expect("committed CSV capture identity")
                            .to_owned(),
                    );
                    output_published = true;
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
            })();
            resolver.close_after(resolved)
        };
        if inv.flag_bool("--cdn") {
            match pin.data.as_ref() {
                Some(data) => scope.with_session(data, |reader| execute_sql(reader)),
                None => Err(fault(
                    "records.data_pin_required",
                    2,
                    "SQL requires a fixed DataPin",
                )),
            }
        } else {
            execute_sql(scope)
        }
    });
    let _phase = budget.phase(Phase::SourceClose);
    scope
        .finish(result)
        .map_err(|error| sql_committed_error(error, &committed, output_published))
}
fn sql_committed_error(
    error: lycheedev_core::ToolkitError,
    committed: &[String],
    output_published: bool,
) -> lycheedev_core::ToolkitError {
    capture::query_committed_error(error, committed, output_published)
}
pub fn asset(inv: &Invocation) -> Result<Outcome> {
    asset_after_finish(inv, &mut |result| result)
}
/// Observe the real checked finish after actual IO/capture. Tests may inject a
/// typed close diagnostic; production forwards the actual owner result unchanged.
pub(crate) fn asset_after_finish(
    inv: &Invocation,
    after_finish: &mut impl FnMut(Result<Outcome>) -> Result<Outcome>,
) -> Result<Outcome> {
    asset_after_closes(inv, after_finish, &mut |result| result)
}
// Narrow close observer, like the existing final-scope observer: production
// forwards the actual checked result. Tests cannot bypass or substitute close.
fn asset_after_closes(
    inv: &Invocation,
    after_finish: &mut impl FnMut(Result<Outcome>) -> Result<Outcome>,
    after_blob_close: &mut impl FnMut(Result<Outcome>) -> Result<Outcome>,
) -> Result<Outcome> {
    inv.control.check()?;
    let id = inv.required("--file-id")?.parse::<u32>().map_err(|_| {
        fault(
            "command.invalid_arguments",
            2,
            "File ID must be a positive u32",
        )
    })?;
    let budget = Arc::new(Budget::query_controlled(
        inv.control.deadline(),
        inv.control.clone(),
    ));
    // Asset reads resolve DataSome before admission, then validate DataIdentity
    // inside the admitted Reader scope, exactly as fixed Go captureAsset does.
    let (mut scope, pin) = {
        let _phase = budget.phase(Phase::Admission);
        crate::remote::provider::OpenedScope::open(inv, &budget)?.admit_asset()?
    };
    let mut committed = None;
    let result = scope.with_capture_metadata(|scope, metadata| {
        let data = pin.data.as_ref().expect("asset admission checked DataPin");
        scope.check_pin(data)?;
        let mut capture = |blob: &mut evidence::VerifiedBlob, reading: &Value| {
            let bytes = usize::try_from(blob.reference().bytes)
                .map_err(|_| fault("records.metadata_limit", 3, "Asset size overflow"))?;
            budget.charge_split(Cost::retained(bytes), Cost::default())?;
            let safety = budget.safety_only();
            // The remote Session keeps its body/provenance; account the distinct
            // returned report before cloning it. The body itself is never copied.
            safety.charge(Cost::metadata(
                capture::json_size(reading, &safety)?.saturating_mul(4),
            ))?;
            let mut reading = reading.clone();
            reading["resourceAccounting"] =
                json!({"wholeQueryAccountingComplete":false,"used":budget.snapshot()?});
            let capture_phase = budget.phase(Phase::SourceCapture);
            let saved = evidence::commit_verified_capture_budgeted_with_metadata(
                blob,
                metadata,
                evidence::CaptureHeader {
                    media_type: "application/octet-stream",
                    provenance: json!({"kind":"asset","locator":format!("casc:fdid:{id}:ckey:{}",reading["entry"]["contentKey"].as_str().unwrap_or("")),"snapshot":pin.id,"dataBuild":data.full_build}),
                    complete: true,
                    truncated: false,
                },
                &inv.control,
                &safety,
            )?;
            drop(capture_phase);
            // A successful commit cannot be rolled back by later Session/owner
            // close failure. Keep its returned identity outside those fallible steps.
            committed = Some(
                saved["id"]
                    .as_str()
                    .expect("committed capture identity")
                    .to_owned(),
            );
            let mut out = Outcome::new(reading);
            out.selection = serde_json::to_value(&pin)?;
            out.provenance = vec![saved];
            Ok(out)
        };
        if inv.flag_bool("--cdn") {
            scope.with_verified_file(data, id, &mut capture)
        } else {
            let mut file = read_verified_file_budgeted(inv, data, id, &budget)?;
            let result = capture(&mut file.blob, &file.reading);
            // Preserve capture failure as primary and any committed identity;
            // successful capture does not excuse an unchecked source close.
            finish_asset_blob(file.blob, result, &budget, after_blob_close)
        }
    });
    let result = {
        let _phase = budget.phase(Phase::SourceClose);
        scope.finish(result)
    };
    after_finish(result).map_err(|mut error| {
        if let Some(id) = committed {
            let primary = error.clone();
            error.details = json!({"schema":"lycheedev.asset-committed-error.v1", "primary":primary,
                "completedCaptures":[id], "outputPublished":false});
            error
                .message
                .push_str("\nasset captures already committed: ");
            error.message.push_str(&id);
        }
        error
    })
}

fn finish_asset_blob(
    blob: evidence::VerifiedBlob,
    result: Result<Outcome>,
    budget: &Budget,
    after_close: &mut impl FnMut(Result<Outcome>) -> Result<Outcome>,
) -> Result<Outcome> {
    let _phase = budget.phase(Phase::SourceClose);
    after_close(blob.close_after(result))
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
        let result = relational::QueryResult {
            plan: None,
            columns: vec!["value".into()],
            rows: vec![vec![relational::Scalar::Float(1.0)]],
            complete: true,
        };
        let budget = Budget::new(
            Cost {
                retained_bytes: (number_tree_bytes::<usize>(1) - 1) as u64,
                ..Cost::defaults()
            },
            Instant::now() + Duration::from_secs(30),
            None,
        );
        assert_eq!(
            public_query_numbers(&request, &result, &budget)
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

#[cfg(test)]
mod partial_tests;
