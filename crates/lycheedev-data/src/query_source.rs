//! Authenticated local source preparation and asset inspection. Raw files are fully CKey checked; tables may
//! explicitly retain authenticated missing-key spans with partial provenance.
//! Encoding claims cover authenticated framing and requested pages only.
use crate::{
    blte::{self, ByteSource},
    capture, casc, decoded_cache, definitions, fault,
    installed, keys,
    metadata_format::{self, Encoding},
    phase_profile::Phase,
    query_value,
    resource::{Budget, Cost},
    schema, table,
};
use crate::request::DataContext;
use lycheedev_core::{Outcome, Result, evidence, selection};
#[cfg(test)]
use std::time::Duration;
use serde_json::{Value, json};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    sync::Arc,
    time::Instant,
};

pub(crate) mod effective;
mod file_bytes;
mod generation_provider;
pub(crate) mod provider;
pub(crate) mod reader;
pub use file_bytes::FileBytes;

/// Fixed Go query-reader limits. The request's selected-content maximum is
/// distinct from the general-purpose WDC SDK defaults and shared query ledger.
pub(crate) fn table_limits(maximum: usize) -> table::TableLimits {
    table::TableLimits {
        file_bytes: maximum,
        metadata_bytes: 64 << 20,
        rows: 1_000_000,
        columns: 4096,
        partitions: 4096,
        ..Default::default()
    }
}

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
/// No IO, allocation admission, key preparation or pin lookup. Keep Go's
/// variant-before-FileQuery order with the existing local multi-ID contract.
fn validate_local_file_request(inv: &DataContext, ids: &[u32]) -> Result<usize> {
    if inv.source.cdn || inv.source.installation.is_none() {
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
    Ok(inv.source.max_file_bytes)
}

/// The callback may request another bounded table batch while the authenticated
/// Encoding/Root session remains alive. Delivered callbacks are never replayed.
pub(crate) fn read_table_batches_budgeted(
    inv: &DataContext,
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
        ReadMode::Materialized(blte::DecodePolicy::TableAllowMissing),
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
    inv: &DataContext,
    pin: &selection::DataPin,
    id: u32,
    budget: &Arc<Budget>,
) -> Result<VerifiedFile> {
    read_verified_file_after_read(inv, pin, id, budget, &mut || Ok(()), &mut |result| result)
}
// Observe only after ownership delivery and after the real batch/reader close.
// Production observers are identities; the actual read and close cannot be skipped.
fn read_verified_file_after_read(
    inv: &DataContext,
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
    inv: &DataContext,
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
    let mut keys = keys::Context::prepare_scoped(&inv.home, inv.source.key_file.as_deref(), inv.source.offline, &inv.control, deadline, Some(budget.clone()))?;
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
    inv: &DataContext,
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
    let variant = inv.source.content_variant.as_str();
    let installation = inv.source.installation.as_deref().expect("validated local source");
    budget.charge(Cost::metadata(
        installation.to_string_lossy().len()
            + pin.product.len()
            + pin.full_build.len()
            + pin.region.len()
            + pin.language.len()
            + pin.definition_commit.len(),
    ))?;
    let target = installed::resolve_pinned_target_budgeted(
        installation,
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
        let mut configuration_blobs: Option<(Value, Value)> = None;
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
                    // Retained wire semantics removed the cumulative raw-content cap. Each file
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
                            let blob = lycheedev_core::workspace::data_disk::publish_shared_blob_with_check(
                                &inv.home,
                                &content.bytes,
                                max,
                                &inv.control, &|| budget.check()).and_then(|blob| serde_json::to_value(blob).map_err(Into::into))?;
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
                    let mut reading = json!({"contentVerified":true,"source":"installation","pin":pin,"catalogSHA256":target.metadata.catalog_sha256,"encodingKey":target.metadata.encoding_key,"root":root_blob,"entry":entry,"payloadEncodingKey":payload_key,"content":content_blob,"verification":{"encodingFramingEKey":true,"encodingRequestedPagesMD5":true,"encodingWholeDecodedCKey":false,"rootWholeDecodedCKey":true,"contentWholeDecodedCKey":true},"encodingIO":reader.io_stats()});
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
                // Exact selected-body recovery must finish before ordinary
                // provenance publication inventories every attributed body.
                // Both modes finish their selected source reads before returning;
                // verified streams have consumed authenticated source EOF, while
                // materialized table reads retain their explicit partial coverage.
                if configuration_blobs.is_none() {
                    let [build_blob, cdn_blob] =
                        lycheedev_core::workspace::data_disk::publish_shared_blob_pair_with_check(
                            &inv.home,
                            [
                                &target.metadata.build_document.raw,
                                &target.metadata.cdn_document.raw,
                            ],
                            4 << 20,
                            &inv.control,
                            &|| budget.check(),
                        )?;
                    configuration_blobs = Some((
                        serde_json::to_value(build_blob)?,
                        serde_json::to_value(cdn_blob)?,
                    ));
                }
                let (build_blob, cdn_blob) = configuration_blobs
                    .as_ref()
                    .expect("published configuration provenance");
                for file in &mut result {
                    file.reading["buildConfiguration"] = build_blob.clone();
                    file.reading["cdnConfiguration"] = cdn_blob.clone();
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
pub fn asset(ctx: &DataContext, file_id: u32) -> Result<Outcome> {
    asset_after_finish(ctx, file_id, &mut |result| result)
}
/// Observe the real checked finish after actual IO/capture. Tests may inject a
/// typed close diagnostic; production forwards the actual owner result unchanged.
pub(crate) fn asset_after_finish(
    ctx: &DataContext,
    file_id: u32,
    after_finish: &mut impl FnMut(Result<Outcome>) -> Result<Outcome>,
) -> Result<Outcome> {
    asset_after_closes(ctx, file_id, after_finish, &mut |result| result)
}
// Narrow close observer, like the existing final-scope observer: production
// forwards the actual checked result. Tests cannot bypass or substitute close.
fn asset_after_closes(
    ctx: &DataContext,
    file_id: u32,
    after_finish: &mut impl FnMut(Result<Outcome>) -> Result<Outcome>,
    after_blob_close: &mut impl FnMut(Result<Outcome>) -> Result<Outcome>,
) -> Result<Outcome> {
    ctx.control.check()?;
    let id = file_id;
    let budget = Arc::new(Budget::query_controlled(
        ctx.control.deadline(),
        ctx.control.clone(),
    ));
    // Asset reads resolve DataSome before admission, then validate DataIdentity
    // inside the admitted Reader scope, exactly as fixed Go captureAsset does.
    let (mut scope, pin) = {
        let _phase = budget.phase(Phase::Admission);
        crate::remote::provider::OpenedScope::open(ctx, &budget)?.admit_asset()?
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
                &ctx.control,
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
        if ctx.source.cdn {
            scope.with_verified_file(data, id, &mut capture)
        } else {
            let mut file = read_verified_file_budgeted(ctx, data, id, &budget)?;
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
        let mut observed = vec![];
        for source in ["public", "file"] {
            let ctx = DataContext {
                home: home.clone(),
                cwd: root.clone(),
                control: Default::default(),
                source: crate::request::SourceOptions {
                    offline: true,
                    installation: Some(std::path::PathBuf::from(identity["installation"].as_str().unwrap())),
                    key_file: (source == "file").then(|| root.join("public-keys.txt").to_string_lossy().into_owned()),
                    ..Default::default()
                },
            };
            let (budget, profile) = Budget::controlled(
                Cost::defaults(),
                Instant::now() + Duration::from_secs(30),
                ctx.control.clone(),
            ).with_test_phase_profile();
            let budget = Arc::new(budget);
            let error = read_verified_file_budgeted(&ctx, &pin, 303, &budget)
                .err()
                .expect("actual encrypted chunk's absent key");
            assert_eq!(error.code, "container.key_unavailable");
            assert_eq!(error.details["missingKey"], true);
            assert_eq!(error.details["keyId"], "000000000000002a");
            let cost = budget.snapshot().unwrap();
            assert_eq!(cost.network_requests, 0);
            assert_eq!(cost.network_bytes, 0);
            drop(budget);
            let mut records = profile.lock().unwrap();
            assert_eq!(records.len(), 1);
            observed.push((source, cost, records.pop().unwrap()));
        }
        // Public loading owns an extra input buffer and retries the selected
        // content reader. Observe the preparation chain directly instead of
        // requiring two different key providers to charge identical bytes.
        let calls = |profile: &Value, group: &str, phase: &str| {
            profile[group].as_array().unwrap().iter()
                .find(|row| row["phase"] == phase).unwrap()["calls"].as_u64().unwrap()
        };
        for (_, _, profile) in &observed {
            assert_eq!(calls(profile, "phases", "prepare_inclusive"), 1, "{profile}");
            assert_eq!(calls(profile, "phases", "payload_inclusive"), 1, "{profile}");
            assert_eq!(calls(profile, "details", "encoded_identity_hash"), 2, "{profile}");
            assert_eq!(calls(profile, "details", "decoded_cache_lookup"), 2, "{profile}");
        }
        assert_eq!(calls(&observed[0].2, "details", "decoded_stream_publish_inclusive"),
            calls(&observed[1].2, "details", "decoded_stream_publish_inclusive") + 1);
        // Aggregate work also includes each key provider's cache path. The
        // exact preparation and content-attempt counts above prove no replay;
        // no ordering of those different aggregate costs is part of that contract.
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
